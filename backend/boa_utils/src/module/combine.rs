use std::{cell::RefCell, rc::Rc};

use boa_engine::{Context, JsResult, JsString, Module, module::ModuleLoader};
use url::Url;

use crate::module::builtin::{BUILTIN_MODULE_PREFIX, BuiltinModuleLoader};

pub struct CombineModuleLoader {
    simple: Rc<boa_engine::module::SimpleModuleLoader>,
    http: Rc<super::http::HttpModuleLoader>,
    builtin: Rc<super::builtin::BuiltinModuleLoader>,
}

impl CombineModuleLoader {
    pub fn new(
        simple: boa_engine::module::SimpleModuleLoader,
        http: super::http::HttpModuleLoader,
    ) -> Self {
        Self {
            simple: Rc::new(simple),
            http: Rc::new(http),
            builtin: Rc::new(BuiltinModuleLoader),
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
    async fn load_imported_module(
        self: Rc<Self>,
        referrer: boa_engine::module::Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let specifier_str = specifier.to_std_string_escaped();
        match Url::parse(&specifier_str) {
            Ok(url) if url.scheme() == "http" || url.scheme() == "https" => {
                self.http
                    .clone()
                    .load_imported_module(referrer, specifier, context)
                    .await
            }
            _ => {
                if specifier_str.starts_with(BUILTIN_MODULE_PREFIX) {
                    self.builtin
                        .clone()
                        .load_imported_module(referrer, specifier, context)
                        .await
                } else {
                    self.simple
                        .clone()
                        .load_imported_module(referrer, specifier, context)
                        .await
                }
            }
        }
    }
}
