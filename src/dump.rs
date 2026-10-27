use crate::{
    addresses::*,
    memory::{Memory, f32_at, u16_at, u32_at},
};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn dump(memory: &impl Memory, kind: char) -> Result<PathBuf, String> {
    let name = if kind == 'e' {
        "entities"
    } else {
        "animations"
    };
    fs::create_dir_all("dumps").map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let path = PathBuf::from(format!("dumps/{name}-{stamp}.txt"));
    let content = if kind == 'e' {
        entities(memory)?
    } else {
        animations(memory)?
    };
    let mut file = File::create(&path).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(path)
}
fn script_string(m: &impl Memory, id: u16) -> Result<String, String> {
    if id == 0 {
        return Ok(String::new());
    }
    m.string(STRINGS + 24 * u32::from(id) + 4)
}
fn entities(m: &impl Memory) -> Result<String, String> {
    use std::fmt::Write;
    let level = m.read(LEVEL, 24)?;
    if m.u32(SERVER_ENTITY_SIZE)? as usize != ENTITY_SIZE {
        return Err("unsupported entity size".into());
    }
    let count = u32_at(&level, 12) as usize;
    if count > 1024 {
        return Err("invalid entity count".into());
    }
    let pool = m.read(u32_at(&level, 4), ENTITY_SIZE * count)?;
    let mut out = String::from("ENTITY INFO DUMP\n");
    let mut used = 0;
    for (i, e) in pool.as_chunks::<ENTITY_SIZE>().0.iter().enumerate() {
        if e[251] == 0 {
            continue;
        }
        used += 1;
        writeln!(out,"\n---------------\nent_num: {i}\nclass: {}\ntype: {}\ntargetname: {}\ntarget: {}\nscript_noteworthy: {}",
            script_string(m,u16_at(e,374))?,u16_at(e,216),script_string(m,u16_at(e,378))?,script_string(m,u16_at(e,376))?,script_string(m,u16_at(e,380))?).unwrap();
    }
    writeln!(out, "\nTotal number of entities in use: {used}").unwrap();
    Ok(out)
}
fn animations(m: &impl Memory) -> Result<String, String> {
    use std::fmt::Write;
    let pool = m.read(ANIMS, ANIM_SIZE * ANIM_LIMIT)?;
    let mut out = String::from("ANIM INFO DUMP\n");
    let mut used = 0;
    for (i, a) in pool.as_chunks::<ANIM_SIZE>().0.iter().enumerate() {
        writeln!(out, "\n---------------\nanim_index: {i}").unwrap();
        if u16_at(a, 18) == 0 || u32_at(a, 20) == 0 {
            out.push_str("status: unused or blend node\n");
            continue;
        }
        // anim.parts is a union: only dereference leaf entries with animToModel set.
        let parts = match m.read(u32_at(a, 20), 64) {
            Ok(p) => p,
            Err(e) => {
                writeln!(out, "status: parts unavailable: {e}").unwrap();
                continue;
            }
        };
        let frequency = f32_at(&parts, 52);
        if frequency.is_finite() && frequency >= 0.01 {
            used += 1;
        }
        writeln!(out,"status: likely used\nanim_name: {}\nnotify_name: {}\nnotify_type: {}\nis_looping: {}\nnotify_count: {}\nnum_frames: {}\nframe_rate: {:.2}\nfrequency: {:.4}\nanim_time: {:.4}\ngoal_time: {:.4}",
            m.string(u32_at(&parts,0))?,script_string(m,u16_at(a,4))?,script_string(m,u16_at(a,6))?,parts[16],parts[34],u16_at(&parts,14),f32_at(&parts,48),frequency,f32_at(a,24),f32_at(a,36)).unwrap();
    }
    writeln!(out, "\n{used} anims in use").unwrap();
    Ok(out)
}
