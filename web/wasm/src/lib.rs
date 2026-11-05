//! WebAssembly bindings for the shared JSON call surface.
//!
//! Build with `wasm-pack build --target web web/wasm --out-dir ../frontend/pkg`.

use alifib::interactive::kernel::{FsAccess, Kernel};
use alifib::interactive::rpc::Rpc;
use wasm_bindgen::prelude::*;

/// The browser's in-memory kernel, driven through the same wire as HTTP.
#[wasm_bindgen]
pub struct WasmKernel {
    rpc: Rpc,
}

#[wasm_bindgen]
impl WasmKernel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmKernel {
        WasmKernel {
            rpc: Rpc::new(
                Kernel::new(FsAccess::Denied("the browser build has no disk")),
                "0".to_owned(),
            ),
        }
    }

    pub fn call(&mut self, request: &str) -> String {
        self.rpc.call(request)
    }
}
