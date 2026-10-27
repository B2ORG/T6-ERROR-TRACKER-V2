use std::io;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::{
            Debug::ReadProcessMemory,
            ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        },
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
    },
};

pub trait Memory {
    fn read(&self, address: u32, size: usize) -> Result<Vec<u8>, String>;
    fn u32(&self, address: u32) -> Result<u32, String> {
        Ok(u32_at(&self.read(address, 4)?, 0))
    }
    fn string(&self, address: u32) -> Result<String, String> {
        if address == 0 {
            return Ok(String::new());
        }
        let mut bytes = Vec::new();
        // Byte reads stop at the terminator, including strings at page boundaries.
        for offset in 0..255 {
            let byte = self.read(
                address
                    .checked_add(offset)
                    .ok_or("string address overflow")?,
                1,
            )?[0];
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
pub fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
pub fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32_at(bytes, offset))
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: Handle owns a valid handle returned by Windows, closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct Process {
    handle: Handle,
    pub pid: u32,
    pub name: String,
}

impl Process {
    pub fn open(pid: u32, name: String) -> Result<Self, String> {
        // SAFETY: No borrowed pointers; Windows validates pid and access rights.
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if handle.is_null() {
            return Err(format!(
                "OpenProcess({pid}): {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self {
            handle: Handle(handle),
            pid,
            name,
        })
    }

    pub fn discover() -> Result<Vec<(u32, String)>, String> {
        // SAFETY: These calls fill an initialized Windows POD structure of dwSize bytes.
        unsafe {
            let raw = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if raw == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error().to_string());
            }
            let snapshot = Handle(raw);
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = Vec::new();
            let mut more = Process32FirstW(snapshot.0, &mut entry);
            while more != 0 {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if [
                    "t6zmv41.exe",
                    "t6zm.exe",
                    "plutonium-bootstrapper-win32.exe",
                ]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
                {
                    found.push((entry.th32ProcessID, name));
                }
                more = Process32NextW(snapshot.0, &mut entry);
            }
            Ok(found)
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        use crate::addresses::*;
        if self.read(0x0040_0000, 2)? != b"MZ" {
            return Err("T6 image not mapped at 0x400000".into());
        }
        let code = self.read(CHECK_OVERFLOW, 512)?;
        // Verify the actual x86 reset predicate before trusting this address profile:
        // mov eax, 0x7ffffffe; sub eax, [numSnapshotEntities]; cmp [nextSnapshotEntities], eax.
        let mut predicate = vec![0xB8, 0xFE, 0xFF, 0xFF, 0x7F, 0x2B, 0x05];
        predicate.extend(NUM_SNAPSHOT_ENTITIES.to_le_bytes());
        predicate.extend([0x39, 0x05]);
        predicate.extend(NEXT_SNAPSHOT_ENTITIES.to_le_bytes());
        if !code.windows(predicate.len()).any(|w| w == predicate) {
            return Err(
                "unsupported T6 build: server reset predicate does not match the address profile"
                    .into(),
            );
        }
        Ok(())
    }
}

impl Memory for Process {
    fn read(&self, address: u32, size: usize) -> Result<Vec<u8>, String> {
        if address == 0 {
            return Err("refusing to read a null remote pointer".into());
        }
        let mut bytes = vec![0; size];
        let mut count = 0;
        // SAFETY: Destination is writable for size bytes. The source is a remote address;
        // ReadProcessMemory validates it, and we require an exact read before decoding.
        let ok = unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as usize as *const _,
                bytes.as_mut_ptr().cast(),
                size,
                &mut count,
            )
        };
        if ok == 0 || count != size {
            return Err(format!(
                "ReadProcessMemory(0x{address:08X}, {size} bytes): {} ({count} bytes read)",
                io::Error::last_os_error()
            ));
        }
        Ok(bytes)
    }
}
