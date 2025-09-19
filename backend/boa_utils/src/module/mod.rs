#![allow(dead_code)]
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use boa_engine::module::ModuleLoader as BoaModuleLoader;
use std::sync::Mutex;
pub mod builtin;
pub mod combine;
pub mod http;

pub struct ModuleLoader(Vec<Rc<dyn BoaModuleLoader>>, Mutex<HashMap<String, usize>>);

auto trait NotModuleLoader {}
impl !NotModuleLoader for ModuleLoader {}

impl<M: BoaModuleLoader + 'static + NotModuleLoader> From<M> for ModuleLoader {
    fn from(m: M) -> Self {
        Self(vec![Rc::new(m)], Mutex::new(HashMap::new()))
    }
}

impl From<Vec<Rc<dyn BoaModuleLoader>>> for ModuleLoader {
    fn from(m: Vec<Rc<dyn BoaModuleLoader>>) -> Self {
        Self(m, Mutex::new(HashMap::new()))
    }
}

impl BoaModuleLoader for ModuleLoader {
    fn load_imported_module(
        &self,
        referrer: boa_engine::module::Referrer,
        specifier: boa_engine::JsString,
        finish_load: Box<
            dyn FnOnce(boa_engine::JsResult<boa_engine::Module>, &mut boa_engine::Context),
        >,
        context: &mut boa_engine::Context,
    ) {
        let result: Rc<RefCell<Option<boa_engine::JsResult<boa_engine::Module>>>> =
            Rc::new(RefCell::new(None));
        let result_ = result.clone();
        let call = move |module: boa_engine::JsResult<boa_engine::Module>,
                         _: &mut boa_engine::Context| {
            *result_.borrow_mut() = Some(module);
        };
        for (index, loader) in self.0.iter().enumerate() {
            loader.load_imported_module(
                referrer.clone(),
                specifier.clone(),
                Box::new(call.clone()),
                context,
            );
            if let Some(res) = result.borrow_mut().take() {
                if res.is_err() && index < self.0.len() - 1 {
                    continue;
                }
                {
                    let mut map = self.1.lock().expect("lock poisoned");
                    map.insert(specifier.to_std_string_escaped(), index);
                }
                finish_load(res, context);
                return;
            }
        }
    }

    fn register_module(&self, _specifier: boa_engine::JsString, _module: boa_engine::Module) {
        let record = {
            let map = self.1.lock().expect("lock poisoned");
            map.get(&_specifier.to_std_string_escaped()).cloned()
        };
        match record {
            Some(index) => unsafe {
                self.0
                    .get_unchecked(index)
                    .register_module(_specifier, _module)
            },
            None => {
                self.0[0].register_module(_specifier, _module);
            }
        }
    }

    fn get_module(&self, _specifier: boa_engine::JsString) -> Option<boa_engine::Module> {
        let record = {
            let map = self.1.lock().expect("lock poisoned");
            map.get(&_specifier.to_std_string_escaped()).cloned()
        };
        match record {
            Some(index) => unsafe { self.0.get_unchecked(index).get_module(_specifier) },
            None => self.0[0].get_module(_specifier),
        }
    }

    fn init_import_meta(
        &self,
        _import_meta: &boa_engine::JsObject,
        _module: &boa_engine::Module,
        _context: &mut boa_engine::Context,
    ) {
        for loader in self.0.iter() {
            loader.init_import_meta(_import_meta, _module, _context);
        }
    }
}
