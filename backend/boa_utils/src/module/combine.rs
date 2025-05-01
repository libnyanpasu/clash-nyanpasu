use std::rc::Rc;

use boa_engine::module::ModuleLoader;
use url::Url;

pub struct CombineModuleLoader {
    simple: Rc<boa_engine::module::SimpleModuleLoader>,
    http: Rc<super::http::HttpModuleLoader>,
}

impl CombineModuleLoader {
    pub fn new(
        simple: boa_engine::module::SimpleModuleLoader,
        http: super::http::HttpModuleLoader,
    ) -> Self {
        Self {
            simple: Rc::new(simple),
            http: Rc::new(http),
        }
    }

    pub fn clone_simple(&self) -> Rc<boa_engine::module::SimpleModuleLoader> {
        self.simple.clone()
    }

    pub fn clone_http(&self) -> Rc<super::http::HttpModuleLoader> {
        self.http.clone()
    }
}

impl ModuleLoader for CombineModuleLoader {
    fn load_imported_module(
        &self,
        referrer: boa_engine::module::Referrer,
        specifier: boa_engine::JsString,
        finish_load: Box<
            dyn FnOnce(boa_engine::JsResult<boa_engine::Module>, &mut boa_engine::Context),
        >,
        context: &mut boa_engine::Context,
    ) {
        let specifier_str = specifier.to_std_string_escaped();
        match Url::parse(&specifier_str) {
            Ok(url) if url.scheme() == "http" || url.scheme() == "https" => {
                self.http
                    .load_imported_module(referrer, specifier, finish_load, context);
            }
            _ => {
                self.simple
                    .load_imported_module(referrer, specifier, finish_load, context);
            }
        }
    }

    fn get_module(&self, _specifier: boa_engine::JsString) -> Option<boa_engine::Module> {
        let specifier_str = _specifier.to_std_string_escaped();
        match Url::parse(&specifier_str) {
            Ok(url) if url.scheme() == "http" || url.scheme() == "https" => {
                self.http.get_module(_specifier)
            }
            _ => self.simple.get_module(_specifier),
        }
    }

    fn register_module(&self, _specifier: boa_engine::JsString, _module: boa_engine::Module) {
        let specifier_str = _specifier.to_std_string_escaped();
        match Url::parse(&specifier_str) {
            Ok(url) if url.scheme() == "http" || url.scheme() == "https" => {
                self.http.register_module(_specifier, _module);
            }
            _ => self.simple.register_module(_specifier, _module),
        }
    }
}
