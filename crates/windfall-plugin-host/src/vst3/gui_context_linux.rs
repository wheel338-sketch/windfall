//! Factory/initialize host context IRunLoop, including before an editor exists.
//! The owning guard stops callbacks and releases handler refs while the module
//! is still mapped; the factory may retain an inert context until its release.
use super::*;
pub(super) struct Owner {
    context: ComPtr<IHostApplication>,
    run_loop: editor::run_loop::RunLoop,
}
impl Owner {
    pub fn new(module: &Vst3Module) -> Result<Self, PluginError> {
        let run_loop = editor::run_loop::RunLoop::new();
        let context = ComWrapper::new(HostApplication {
            run_loop: run_loop.clone(),
        })
        .to_com_ptr::<IHostApplication>()
        .expect("Linux host context");
        let owner = Self { context, run_loop };
        module.with_factory(|factory| {
            if let Some(factory) = factory.cast::<IPluginFactory3>()
                && unsafe { factory.setHostContext(owner.context.as_ptr().cast()) } != kResultOk
            {
                return Err(PluginError::Create(
                    "VST3 factory refused Linux GUI context".into(),
                ));
            }
            Ok(())
        })?;
        Ok(owner)
    }
    pub fn context(&self) -> ComPtr<IHostApplication> {
        Clone::clone(&self.context)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.run_loop.clear();
    }
}
