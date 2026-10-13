//! VST3 module loading, scanning and hosting through the pinned SDK bindings.
mod buffers;
#[cfg(target_os = "macos")]
pub(crate) mod bundle_macos;
mod editor;
#[cfg(target_os = "linux")]
mod gui_context_linux;
mod handlers;
mod instance;
#[cfg(any(target_os = "macos", test))]
mod lifecycle;
mod processor;
mod stream;
use crate::descriptor::{
    AudioPort, MAX_PARAMETERS, MAX_PORT_CHANNELS, MAX_PORTS, PluginDescriptor, PluginFormat,
    PluginKind, PluginLayout,
};
use crate::error::PluginError;
use crate::instance::InstanceBackend;
#[cfg(not(target_os = "macos"))]
use libloading::Library;
use std::ffi::c_void;
use std::path::Path;
use std::ptr;
use vst3::Steinberg::Vst::*;
use vst3::{Class, ComPtr, ComWrapper, Interface, Steinberg::*};

#[derive(Clone)]
pub(crate) struct Vst3Module {
    #[cfg(target_os = "macos")]
    inner: std::rc::Rc<ModuleData>,
    #[cfg(not(target_os = "macos"))]
    inner: std::sync::Arc<ModuleData>,
}
pub(crate) struct ModuleData {
    path: std::path::PathBuf,
    #[cfg(target_os = "macos")]
    owner: bundle_macos::MacModuleLease,
    #[cfg(not(target_os = "macos"))]
    factory: Option<ComPtr<IPluginFactory>>,
    #[cfg(not(target_os = "macos"))]
    library: Library,
    #[cfg(not(target_os = "macos"))]
    exit: Option<unsafe extern "system" fn() -> bool>,
}
impl std::ops::Deref for Vst3Module {
    type Target = ModuleData;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
fn text(bytes: &[char8]) -> String {
    let bytes: Vec<u8> = bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
fn class_id(cid: &TUID) -> String {
    // Windows uses COM byte order for the first three GUID fields. Store
    // SDK FUID text so project identifiers travel between platforms.
    let mut cid = *cid;
    canonical_order(&mut cid);
    cid.iter()
        .map(|byte| format!("{:02X}", *byte as u8))
        .collect()
}
fn parse_id(id: &str) -> Option<TUID> {
    if id.len() != 32 || !id.is_ascii() {
        return None;
    }
    let mut cid = [0; 16];
    for (i, byte) in cid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&id[i * 2..i * 2 + 2], 16).ok()? as char8;
    }
    canonical_order(&mut cid);
    Some(cid)
}
fn canonical_order(cid: &mut TUID) {
    if cfg!(windows) {
        cid[..4].reverse();
        cid[4..6].reverse();
        cid[6..8].reverse();
    }
}
impl Vst3Module {
    pub fn load(_path: &Path) -> Result<Self, PluginError> {
        #[cfg(target_os = "macos")]
        {
            let owner = bundle_macos::MacModuleLease::load(_path)?;
            Ok(Self {
                inner: std::rc::Rc::new(ModuleData { owner, path: _path.to_owned() }),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let path = _path;
            let binary = crate::paths::vst3_binary(path)
                .ok_or_else(|| PluginError::Load("no VST3 binary for this architecture".into()))?;
            let binary = binary
                .canonicalize()
                .map_err(|error| PluginError::Load(error.to_string()))?;
            // Module entry/exit are per DLL, not per PluginHost::load. This
            // cache is used only by main-thread loading; audio never visits
            // it or takes its mutex. Weak refs do not extend DLL lifetime.
            type Cache = std::collections::HashMap<std::path::PathBuf, std::sync::Weak<ModuleData>>;
            static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
            let mut cache = CACHE
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| PluginError::Load("VST3 module cache poisoned".into()))?;
            if let Some(inner) = cache.get(&binary).and_then(std::sync::Weak::upgrade) {
                return Ok(Self { inner });
            }
            // SAFETY: loading runs plugin code. The scanner isolates crashes.
            let library = unsafe { Library::new(&binary) }
                .map_err(|error| PluginError::Load(error.to_string()))?;
            #[cfg(not(target_os = "linux"))]
            let mut exit = None;
            #[cfg(target_os = "linux")]
            let exit;
            #[cfg(windows)]
            // SAFETY: SDK module exports have these signatures.
            unsafe {
                if let Ok(entry) = library.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0")
                {
                    if !entry() {
                        return Err(PluginError::Load("InitDll refused".into()));
                    }
                    exit = library
                        .get::<unsafe extern "system" fn() -> bool>(b"ExitDll\0")
                        .ok()
                        .map(|f| *f);
                }
            }
            #[cfg(target_os = "linux")]
            let library = {
                let platform: libloading::os::unix::Library = library.into();
                let raw = platform.into_raw();
                // SAFETY: ownership of this dlopen handle is restored once.
                let library: Library =
                    unsafe { libloading::os::unix::Library::from_raw(raw) }.into();
                // SAFETY: SDK Linux entry takes the module's dlopen handle.
                unsafe {
                    if let Ok(entry) =
                        library.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"ModuleEntry\0")
                        && !entry(raw)
                    {
                        return Err(PluginError::Load("ModuleEntry refused".into()));
                    }
                    exit = library
                        .get::<unsafe extern "system" fn() -> bool>(b"ModuleExit\0")
                        .ok()
                        .map(|f| *f);
                }
                library
            };
            // SAFETY: SDK export returns an owned reference, kept alive by
            // `library` until Drop releases the factory before unloading.
            // Once module entry succeeds, every later failure must still
            // run module exit before unloading the library.
            let mut module = ModuleData {
                path: path.to_owned(),
                factory: None,
                library,
                exit,
            };
            let factory = unsafe {
                let get = module
                    .library
                    .get::<unsafe extern "system" fn() -> *mut IPluginFactory>(
                        b"GetPluginFactory\0",
                    )
                    .map_err(|error| PluginError::Load(error.to_string()))?;
                ComPtr::from_raw(get()).ok_or_else(|| PluginError::Load("null factory".into()))?
            };
            module.factory = Some(factory);
            let inner = std::sync::Arc::new(module);
            cache.retain(|_, module| module.strong_count() > 0);
            cache.insert(binary, std::sync::Arc::downgrade(&inner));
            Ok(Self { inner })
        }
    }
    fn with_factory<R>(&self, operation: impl FnOnce(&ComPtr<IPluginFactory>) -> R) -> R {
        #[cfg(target_os = "macos")]
        {
            self.owner.with_factory(operation)
        }
        #[cfg(not(target_os = "macos"))]
        {
            operation(self.factory.as_ref().expect("loaded factory"))
        }
    }
    pub fn descriptors(&self) -> Vec<PluginDescriptor> {
        // SAFETY: live COM references, sized output structures, owning thread.
        self.with_factory(|factory| unsafe {
            let mut info: PFactoryInfo = std::mem::zeroed();
            factory.getFactoryInfo(&mut info);
            let factory2 = factory.cast::<IPluginFactory2>();
            (0..factory.countClasses().clamp(0, MAX_PARAMETERS as i32))
                .filter_map(|index| {
                    let mut basic: PClassInfo = std::mem::zeroed();
                    if factory.getClassInfo(index, &mut basic) != kResultOk
                        || text(&basic.category) != "Audio Module Class"
                    {
                        return None;
                    }
                    let mut descriptor = PluginDescriptor {
                        format: PluginFormat::Vst3,
                        id: class_id(&basic.cid),
                        name: text(&basic.name),
                        vendor: text(&info.vendor),
                        version: String::new(),
                        features: vec!["Fx".into()],
                        kind: PluginKind::Effect,
                    };
                    if let Some(factory2) = &factory2 {
                        let mut extended: PClassInfo2 = std::mem::zeroed();
                        if factory2.getClassInfo2(index, &mut extended) == kResultOk {
                            descriptor.features = text(&extended.subCategories)
                                .split('|')
                                .map(str::to_owned)
                                .collect();
                            descriptor.kind = PluginKind::from_features(
                                descriptor.features.iter().map(String::as_str),
                            );
                            if !text(&extended.vendor).is_empty() {
                                descriptor.vendor = text(&extended.vendor);
                            }
                            descriptor.version = text(&extended.version);
                        }
                    }
                    Some(descriptor)
                })
                .collect()
        })
    }
    pub fn create(&self, id: &str) -> Result<Box<dyn InstanceBackend>, PluginError> {
        Ok(Box::new(instance::VstInstance::new(self.clone(), id)?))
    }
    fn object<T: Interface>(&self, cid: &TUID) -> Result<ComPtr<T>, PluginError> {
        let mut raw = ptr::null_mut();
        // SAFETY: sixteen-byte ids and SDK output pointer for an owned ref.
        self.with_factory(|factory| unsafe {
            let result = factory.createInstance(cid.as_ptr(), T::IID.as_ptr().cast(), &mut raw);
            if result != kResultOk {
                return Err(PluginError::Create(format!("factory returned {result}")));
            }
            ComPtr::from_raw(raw.cast()).ok_or_else(|| PluginError::Create("null instance".into()))
        })
    }
    pub fn probe(&self, id: &str) -> Result<PluginLayout, PluginError> {
        let cid = parse_id(id).ok_or_else(|| PluginError::NotFound(id.into()))?;
        #[cfg(target_os = "linux")]
        let gui_context = gui_context_linux::Owner::new(self)?;
        #[cfg(target_os = "linux")]
        let context = gui_context.context();
        let component = self.object::<IComponent>(&cid)?;
        #[cfg(not(target_os = "linux"))]
        let context = ComWrapper::new(HostApplication)
            .to_com_ptr::<IHostApplication>()
            .expect("host interface");
        // SAFETY: live context/component, held through termination below.
        if unsafe { component.initialize(context.as_ptr().cast()) } != kResultOk {
            return Err(PluginError::Create("component initialize refused".into()));
        }
        let component = Initialized(component);
        let mut layout = PluginLayout {
            has_state: true,
            ..PluginLayout::default()
        };
        for (direction, ports) in [
            (0, &mut layout.audio_inputs),
            (1, &mut layout.audio_outputs),
        ] {
            // SAFETY: live initialized component and SDK media/direction values.
            let count = unsafe { component.0.getBusCount(0, direction) };
            if !(0..=MAX_PORTS as i32).contains(&count) {
                return Err(PluginError::Layout("absurd bus count".into()));
            }
            for index in 0..count {
                // SAFETY: sized SDK output structure and valid bus index.
                let mut bus: BusInfo = unsafe { std::mem::zeroed() };
                // SAFETY: as above; the component owns the queried bus.
                if unsafe { component.0.getBusInfo(0, direction, index, &mut bus) } != kResultOk
                    || !(0..=MAX_PORT_CHANNELS as i32).contains(&bus.channelCount)
                {
                    return Err(PluginError::Layout("invalid bus".into()));
                }
                let end = bus
                    .name
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(bus.name.len());
                ports.push(AudioPort {
                    name: String::from_utf16_lossy(&bus.name[..end]),
                    channels: bus.channelCount as u32,
                    main: bus.busType == 0,
                });
            }
        }
        // SAFETY: initialized component and valid event media/direction values.
        unsafe {
            let inputs = component.0.getBusCount(1, 0);
            let outputs = component.0.getBusCount(1, 1);
            if !(0..=MAX_PORTS as i32).contains(&inputs)
                || !(0..=MAX_PORTS as i32).contains(&outputs)
            {
                return Err(PluginError::Layout("absurd event bus count".into()));
            }
            layout.note_inputs = inputs as u32;
            layout.note_outputs = outputs as u32;
        }
        let mut separate = None;
        let controller = if let Some(controller) = component.0.cast::<IEditController>() {
            Some(controller)
        } else {
            let mut cid = [0; 16];
            // SAFETY: live component and sixteen-byte class-id output.
            let controller_result = unsafe { component.0.getControllerClassId(&mut cid) };
            if controller_result != kResultOk {
                None
            } else {
                let controller = self.object::<IEditController>(&cid)?;
                // SAFETY: live controller and context, retained through terminate.
                if unsafe { controller.initialize(context.as_ptr().cast()) } != kResultOk {
                    return Err(PluginError::Create("controller initialize refused".into()));
                }
                separate = Some(Initialized(controller.clone()));
                Some(controller)
            }
        };
        if let Some(controller) = controller {
            let connection = Connection::new(&component.0, &controller);
            // SAFETY: initialized controller; view reference released at once.
            unsafe {
                let count = controller.getParameterCount();
                if !(0..=MAX_PARAMETERS as i32).contains(&count) {
                    return Err(PluginError::Layout("absurd parameter count".into()));
                }
                layout.parameter_count = count as u32;
                layout.has_editor =
                    ComPtr::from_raw(controller.createView(c"editor".as_ptr())).is_some();
            }
            drop(connection);
        }
        drop(separate);
        Ok(layout)
    }
}
struct Initialized<T: Interface + vst3::com_scrape_types::Inherits<IPluginBase>>(ComPtr<T>);
impl<T: Interface + vst3::com_scrape_types::Inherits<IPluginBase>> Initialized<T> {
    fn into_inner(self) -> ComPtr<T> {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: move the reference once, suppressing only terminate.
        unsafe { std::ptr::read(&this.0) }
    }
}
struct Connection(Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>);
impl Connection {
    fn new(component: &ComPtr<IComponent>, controller: &ComPtr<IEditController>) -> Self {
        let pair = component
            .cast::<IConnectionPoint>()
            .zip(controller.cast::<IConnectionPoint>())
            // Combined component/controllers need no self-connection.
            .filter(|(component, controller)| component.as_ptr() != controller.as_ptr());
        if let Some((component, controller)) = &pair {
            // SAFETY: both initialized objects stay alive until disconnect.
            unsafe {
                component.connect(controller.as_ptr());
                controller.connect(component.as_ptr());
            }
        }
        Self(pair)
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some((component, controller)) = &self.0 {
            // SAFETY: live connected objects; disconnect before termination.
            unsafe {
                component.disconnect(controller.as_ptr());
                controller.disconnect(component.as_ptr());
            }
        }
    }
}
impl<T: Interface + vst3::com_scrape_types::Inherits<IPluginBase>> Drop for Initialized<T> {
    fn drop(&mut self) {
        // SAFETY: this wrapper is only made after successful initialization.
        unsafe {
            self.0.terminate();
        }
    }
}
#[cfg(not(target_os = "macos"))]
impl Drop for ModuleData {
    fn drop(&mut self) {
        drop(self.factory.take());
        // SAFETY: references are gone, export still loaded in `library`.
        if let Some(exit) = self.exit {
            unsafe {
                exit();
            }
        }
        let _ = &self.library;
    }
}
#[cfg(not(target_os = "linux"))]
struct HostApplication;
#[cfg(target_os = "linux")]
struct HostApplication {
    run_loop: editor::run_loop::RunLoop,
}
impl Class for HostApplication {
    #[cfg(not(target_os = "linux"))]
    type Interfaces = (IHostApplication,);
    #[cfg(target_os = "linux")]
    type Interfaces = (IHostApplication, vst3::Steinberg::Linux::IRunLoop);
}
#[cfg(target_os = "linux")]
pub(crate) fn pump_run_loops() {
    editor::run_loop::pump();
}
#[allow(non_snake_case)]
impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        let mut value = [0; 128];
        for (slot, unit) in value.iter_mut().zip("Windfall".encode_utf16()) {
            *slot = unit;
        }
        // SAFETY: SDK caller supplies writable String128 storage.
        unsafe {
            name.write(value);
        }
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if !obj.is_null() {
            // SAFETY: SDK caller supplies a writable output pointer.
            unsafe {
                obj.write(ptr::null_mut());
            }
        }
        kNoInterface
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_ids_use_the_same_text_on_every_platform() {
        let id = vst3::uid(0xabcdef01, 0x12345678, 0x09abcdef, 0x87654321);
        let text = "ABCDEF011234567809ABCDEF87654321";
        assert_eq!(class_id(&id), text);
        assert_eq!(parse_id(text), Some(id));
        assert_eq!(parse_id("not an id"), None);
    }
}
