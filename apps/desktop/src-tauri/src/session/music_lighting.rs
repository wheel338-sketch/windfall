//! Razer's public local REST protocol; no vendor libraries or audio-thread IO.
//! https://assets.razerzone.com/dev_portal/REST/html/index.html
use super::Session;
use crate::sync::lock;
use serde::Serialize;
use windfall_dsp::ParamSet;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const ENDPOINT: &str = "http://127.0.0.1:54235/razer/chromasdk";
const DEVICES: [&str; 6] = ["keyboard", "mouse", "mousepad", "headset", "keypad", "chromalink"];
const HEARTBEAT_MS: u64 = 1000;
const EFFECT_MS: u64 = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightingStatus { pub connected: bool, pub message: String }
impl Default for LightingStatus {
    fn default() -> Self { Self { connected: false, message: "Add Music lighting to a mixer track to connect to the local Chroma REST service.".into() } }
}
#[derive(Debug, Clone, Copy)]
struct Color { device: i32, rgb: [u8; 3] }

fn loopback(value: &str) -> Result<reqwest::Url, String> {
    let url=reqwest::Url::parse(value).map_err(|_| "Chroma returned an invalid session URI")?;
    let local=matches!(url.host(),Some(url::Host::Domain("localhost"))) ||
        matches!(url.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback()) ||
        matches!(url.host(),Some(url::Host::Ipv6(ip)) if ip.is_loopback());
    if !local || url.scheme()!="http" || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err("Chroma session URI must be an HTTP loopback address".into());
    }
    Ok(url)
}

struct Adapter { client: reqwest::Client, endpoint: reqwest::Url, uri: Option<String> }
impl Adapter {
    fn new(endpoint:&str)->Result<Self,String> {
        Ok(Self { endpoint:loopback(endpoint)?,uri:None,client:reqwest::Client::builder()
            .no_proxy().redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_millis(250)).timeout(Duration::from_millis(500))
            .build().map_err(|e|format!("Could not initialise Chroma HTTP client: {e}"))? })
    }
    fn request(&self,method:reqwest::Method,url:&str,body:Option<Value>)->Result<Value,String> {
        let url=loopback(url)?;
        let body=body.map(|body|serde_json::to_vec(&body)).transpose().map_err(|e|e.to_string())?;
        tauri::async_runtime::block_on(async {
            let mut request=self.client.request(method,url);
            if let Some(body)=body {request=request.header(reqwest::header::CONTENT_TYPE,"application/json").body(body);}
            let mut response=request.send().await.map_err(|e|format!("Chroma REST service unavailable: {e}. Enable the local Chroma SDK service."))?;
            if !response.status().is_success(){return Err(format!("Chroma REST service returned HTTP {}",response.status()));}
            let mut bytes=Vec::new();
            while let Some(chunk)=response.chunk().await.map_err(|e|format!("Chroma response failed: {e}"))? {
                if bytes.len().saturating_add(chunk.len())>4096{return Err("Chroma response exceeds 4096 bytes".into());}
                bytes.extend_from_slice(&chunk);
            }
            let value:Value=serde_json::from_slice(&bytes).map_err(|_|"Chroma returned invalid JSON")?;
            if value.get("result").is_some_and(|result|result.as_i64()!=Some(0)){return Err(format!("Chroma rejected the request: {}",value["result"]));}
            Ok(value)
        })
    }
    fn init(&mut self)->Result<(),String> {
        let response=self.request(reqwest::Method::POST,self.endpoint.as_str(),Some(json!({
            "title":"Windfall music lighting","description":"Mixer tempo and audio energy lighting",
            "author":{"name":"Windfall","contact":"Windfall desktop"},"device_supported":DEVICES,"category":"application"
        })))?;
        if response["sessionid"].as_u64().is_none(){return Err("Chroma did not return a session identifier".into());}
        let uri=response["uri"].as_str().ok_or("Chroma did not return a session URI")?;
        self.uri=Some(loopback(uri)?.as_str().trim_end_matches('/').to_owned());Ok(())
    }
    fn heartbeat(&self)->Result<(),String> {
        let uri=self.uri.as_ref().ok_or("Chroma session is not initialised")?;
        let response=self.request(reqwest::Method::PUT,&format!("{uri}/heartbeat"),None)?;
        if response["tick"].as_u64().is_none(){return Err("Chroma heartbeat did not return its tick".into());}Ok(())
    }
    fn effect(&self,device:usize,rgb:Option<[u8;3]>)->Result<(),String> {
        let uri=self.uri.as_ref().ok_or("Chroma session is not initialised")?;
        let body=if let Some([red,green,blue])=rgb {json!({"effect":"CHROMA_STATIC","param":{"color":u32::from(red)|(u32::from(green)<<8)|(u32::from(blue)<<16)}})}else{json!({"effect":"CHROMA_NONE"})};
        let response=self.request(reqwest::Method::PUT,&format!("{uri}/{}",DEVICES[device]),Some(body))?;
        if response["result"].as_i64()!=Some(0){return Err("Chroma effect response has no success result".into());}Ok(())
    }
    fn close(&mut self)->Result<(),String> {
        let Some(uri)=self.uri.take() else{return Ok(());};
        let response=self.request(reqwest::Method::DELETE,&uri,None)?;
        if response["result"].as_i64()!=Some(0){return Err("Chroma uninitialisation did not acknowledge success".into());}Ok(())
    }
}
impl Drop for Adapter { fn drop(&mut self){let _=self.close();} }

struct Driver {
    adapter: Adapter, last_heartbeat:u64,last_effect:Option<u64>,retry_after:u64,
    colors:[Option<[u8;3]>;6],
}
impl Driver {
    fn new(endpoint:&str)->Result<Self,String>{Ok(Self {adapter:Adapter::new(endpoint)?,last_heartbeat:0,last_effect:None,retry_after:0,colors:[None;6]})}
    fn stop(&mut self)->Result<(),String> {
        let mut error=None;
        if self.adapter.uri.is_some(){for (device,color) in self.colors.iter().enumerate(){if color.is_some() && let Err(reason)=self.adapter.effect(device,None){error=Some(reason);}}}
        if let Err(reason)=self.adapter.close(){error=Some(reason);}
        self.colors=[None;6];self.last_effect=None;error.map_or(Ok(()),Err)
    }
    fn tick(&mut self,now:u64,wanted:&[Color])->Result<bool,String> {
        if wanted.is_empty(){self.stop()?;return Ok(false);}
        if self.adapter.uri.is_none(){
            if now<self.retry_after{return Ok(false);}
            self.adapter.init()?;self.last_heartbeat=now;self.last_effect=None;
        }
        if now.saturating_sub(self.last_heartbeat)>=HEARTBEAT_MS {self.adapter.heartbeat()?;self.last_heartbeat=now;}
        if self.last_effect.is_some_and(|last|now.saturating_sub(last)<EFFECT_MS){return Ok(true);}
        let mut colors:[Option<[u8;3]>;6]=[None;6];
        for wanted in wanted {for (device,target) in colors.iter_mut().enumerate(){
            if wanted.device==0 || wanted.device==device as i32+1 {
                let previous=target.unwrap_or([0;3]);*target=Some(std::array::from_fn(|i|previous[i].max(wanted.rgb[i])));
            }
        }}
        for (device,color) in colors.iter().copied().enumerate(){if color!=self.colors[device]{self.adapter.effect(device,color)?;}}
        self.colors=colors;self.last_effect=Some(now);Ok(true)
    }
}
impl Drop for Driver {fn drop(&mut self){let _=self.stop();}}

struct Worker { sender:Option<mpsc::SyncSender<Vec<Color>>>, thread:Option<JoinHandle<()>> }
impl Drop for Worker {fn drop(&mut self){self.sender.take();if let Some(thread)=self.thread.take(){let _=thread.join();}}}
pub(super) struct Service { worker:Mutex<Option<Worker>>,status:Arc<Mutex<LightingStatus>>,endpoint:String }
impl Default for Service {fn default()->Self{Self{worker:Mutex::new(None),status:Arc::new(Mutex::new(LightingStatus::default())),endpoint:ENDPOINT.into()}}}
impl Service {
    fn publish(&self,colors:Vec<Color>) {
        let mut worker=lock(&self.worker);
        if worker.is_none() && colors.is_empty(){return;}
        if worker.is_none(){
            let (sender,receiver)=mpsc::sync_channel::<Vec<Color>>(1);let status=self.status.clone();let endpoint=self.endpoint.clone();
            *lock(&status)=LightingStatus{connected:false,message:"Connecting to local Chroma REST service".into()};
            let thread=std::thread::Builder::new().name("windfall-chroma-rest".into()).spawn(move||{
                let mut driver=match Driver::new(&endpoint){Ok(driver)=>driver,Err(message)=>{*lock(&status)=LightingStatus{connected:false,message};return;}};
                let start=Instant::now();let mut colors=Vec::new();
                loop {
                    match receiver.recv_timeout(Duration::from_millis(EFFECT_MS)){
                        Ok(next)=>{colors=next;while let Ok(next)=receiver.try_recv(){colors=next;}},
                        Err(mpsc::RecvTimeoutError::Timeout)=>{},Err(mpsc::RecvTimeoutError::Disconnected)=>break,
                    }
                    let now=start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                    match driver.tick(now,&colors){
                        Ok(true)=>*lock(&status)=LightingStatus{connected:true,message:"Connected to local Chroma REST service".into()},
                        Ok(false)=>{if colors.is_empty(){*lock(&status)=LightingStatus::default();}},
                        Err(message)=>{let _=driver.stop();driver.retry_after=now+2000;*lock(&status)=LightingStatus{connected:false,message};},
                    }
                }
                let _=driver.stop();
            });
            match thread {Ok(thread)=>*worker=Some(Worker{sender:Some(sender),thread:Some(thread)}),Err(error)=>{*lock(&self.status)=LightingStatus{connected:false,message:format!("Could not start Chroma worker: {error}")};return;}}
        }
        if let Some(worker)=worker.as_ref() && let Some(sender)=&worker.sender {let _=sender.try_send(colors);}
    }
}
fn colors_for_project(project:&windfall_project::Project,frame:&windfall_ipc::RealtimeFrame,head:&windfall_engine::Playhead)->Vec<Color> {
    let phase=(head.tick/f64::from(windfall_core::PPQ)).fract() as f32;
    project.mixer.tracks.iter().enumerate().flat_map(|(track_index,track)|{
        let clean=|value:f32|if value.is_finite(){value.clamp(0.0,1.0)}else{0.0};
        let left=clean(frame.meters.get(track_index*2).copied().unwrap_or(0.0));let right=clean(frame.meters.get(track_index*2+1).copied().unwrap_or(0.0));
        let energy=(left*left+right*right)*0.5;
        track.effects.iter().filter_map(move|slot|{
            let windfall_dsp::EffectParams::MusicLighting(mut params)=slot.params else{return None;};
            if !slot.enabled{return None;}
            let mut mix=slot.mix;
            for value in &frame.automated {
                let target=project.automations.iter().find(|automation|automation.id==value.automation).map(|automation|&automation.target)
                    .or_else(||project.patterns.iter().flat_map(|pattern|&pattern.events).find(|lane|lane.id==value.automation).map(|lane|&lane.target));
                match target {
                    Some(windfall_project::AutomationTarget::EffectParam {track:owner,effect,param}) if *owner==track.id && *effect==slot.id => {
                        if let Some(info)=windfall_dsp::MusicLightingParams::descriptors().get(*param as usize) { params.set(*param as usize,info.min+(info.max-info.min)*clean(value.value)); }
                    }
                    Some(windfall_project::AutomationTarget::EffectMix {track:owner,effect}) if *owner==track.id && *effect==slot.id => mix=clean(value.value),
                    _=>{},
                }
            }
            params.level*=mix;Some(Color{device:params.device,rgb:params.color(phase,energy,head.playing)})
        })
    }).collect()
}
impl Session {
    pub fn music_lighting_status(&self)->LightingStatus{lock(&self.inner.music_lighting.status).clone()}
    pub(super) fn music_lighting_tick(&self,frame:&windfall_ipc::RealtimeFrame){
        let head=self.controller().playhead();
        let colors={let state=self.state();colors_for_project(state.document.project(),frame,&head)};
        self.inner.music_lighting.publish(colors);
    }
}

#[cfg(test)]
mod tests;
