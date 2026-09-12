use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub content: String,
    pub score: f32,
}

#[macro_export]
macro_rules! export_expert_plugin {
    ($plugin:ty) => {
        #[no_mangle]
        pub extern "C" fn manifest() -> *mut u8 {
            let manifest = <$plugin as ExpertPlugin>::manifest();
            let json = serde_json::to_string(&manifest).unwrap();
            let len = json.len();
            let ptr = alloc(len + 8);
            unsafe {
                let len_bytes = len.to_le_bytes();
                std::ptr::copy_nonoverlapping(len_bytes.as_ptr(), ptr, 8);
                std::ptr::copy_nonoverlapping(json.as_ptr(), ptr.add(8), len);
            }
            ptr
        }

        /// WASM 宿主调用入口：`input_ptr` / `input_len` 描述一段宿主内存。
        ///
        /// 标记为 `unsafe extern "C"`：本函数会解引用宿主传入的裸指针，
        /// 安全性由调用方（WASM host）保证。ABI 与 `extern "C" fn` 完全一致，
        /// 不影响导出符号。
        #[no_mangle]
        pub unsafe extern "C" fn run(input_ptr: *const u8, input_len: usize) -> *mut u8 {
            let input = std::slice::from_raw_parts(input_ptr, input_len);
            let input_str = std::str::from_utf8(input).unwrap();
            let response = <$plugin as ExpertPlugin>::run(input_str);
            let len = response.len();
            let ptr = alloc(len + 8);
            unsafe {
                let len_bytes = len.to_le_bytes();
                std::ptr::copy_nonoverlapping(len_bytes.as_ptr(), ptr, 8);
                std::ptr::copy_nonoverlapping(response.as_ptr(), ptr.add(8), len);
            }
            ptr
        }

        #[no_mangle]
        pub extern "C" fn on_activate() -> *mut u8 {
            match <$plugin as ExpertPlugin>::on_activate() {
                Ok(_) => create_response("OK"),
                Err(e) => create_response(&format!("Error: {}", e)),
            }
        }

        #[no_mangle]
        pub extern "C" fn on_deactivate() -> *mut u8 {
            match <$plugin as ExpertPlugin>::on_deactivate() {
                Ok(_) => create_response("OK"),
                Err(e) => create_response(&format!("Error: {}", e)),
            }
        }

        fn create_response(s: &str) -> *mut u8 {
            let len = s.len();
            let ptr = alloc(len + 8);
            unsafe {
                let len_bytes = len.to_le_bytes();
                std::ptr::copy_nonoverlapping(len_bytes.as_ptr(), ptr, 8);
                std::ptr::copy_nonoverlapping(s.as_ptr(), ptr.add(8), len);
            }
            ptr
        }

        static mut HEAP: [u8; 1024 * 1024] = [0; 1024 * 1024];
        static mut HEAP_PTR: usize = 0;

        #[no_mangle]
        pub extern "C" fn alloc(size: usize) -> *mut u8 {
            unsafe {
                let ptr = HEAP_PTR;
                HEAP_PTR += size;
                HEAP.as_mut_ptr().add(ptr)
            }
        }

        #[no_mangle]
        pub extern "C" fn reset() {
            unsafe {
                HEAP_PTR = 0;
            }
        }
    };
}

pub trait ExpertPlugin {
    fn manifest() -> PluginManifest;
    fn run(input: &str) -> String;
    fn on_activate() -> Result<(), String> {
        Ok(())
    }
    fn on_deactivate() -> Result<(), String> {
        Ok(())
    }
}
