use crate::{
    addresses::*,
    memory::{Memory, u16_at, u32_at},
    options::Options,
};
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub struct Reset {
    pub snapshot_entities: u32,
    pub next: u32,
    pub capacity: u32,
    pub threshold: u32,
    pub remaining: u32,
}
impl Reset {
    pub fn new(next: u32, capacity: u32) -> Result<Self, String> {
        if capacity == 0 || capacity >= 2_147_483_646 || next > i32::MAX as u32 {
            return Err(format!(
                "invalid server snapshot counters: next={next}, capacity={capacity}"
            ));
        }
        let threshold = 2_147_483_646 - capacity;
        Ok(Self {
            snapshot_entities: 0,
            next,
            capacity,
            threshold,
            remaining: threshold.saturating_sub(next),
        })
    }
}

fn server_snapshot(memory: &impl Memory) -> Result<Reset, String> {
    // SV_SaveSnapshotForTime stores num_entities at +72 and firstEntityIndex at
    // +84 of each 100-byte serverSnapshot_t. The producer advances a 16-slot ring.
    // Read the ring and cumulative counters together: polling deltas would scale
    // with -t and would not reproduce the per-snapshot value in Plutonium's HUD.
    let size = (NEXT_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize + 4;
    let block = memory.read(SERVER_SNAPSHOTS, size)?;
    let index_offset = (NEXT_SERVER_SNAPSHOT - SERVER_SNAPSHOTS) as usize;
    let next_index = u32_at(&block, index_offset);
    if next_index >= SERVER_SNAPSHOT_COUNT as u32 {
        return Err("invalid server snapshot ring index".into());
    }
    let latest = (next_index as usize + SERVER_SNAPSHOT_COUNT - 1) % SERVER_SNAPSHOT_COUNT;
    let frame_offset = latest * SERVER_SNAPSHOT_SIZE;
    let count = u32_at(&block, frame_offset + 72);
    let first_entity = u32_at(&block, frame_offset + 84);
    let mut reset = Reset::new(
        u32_at(&block, (NEXT_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize),
        u32_at(&block, (NUM_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize),
    )?;
    if count > 1024
        || first_entity.checked_add(count) != Some(reset.next)
        || memory.u32(NEXT_SERVER_SNAPSHOT)? != next_index
    {
        return Err("server snapshot changed during sampling; retrying".into());
    }
    reset.snapshot_entities = count;
    Ok(reset)
}

#[derive(Default, Debug)]
pub struct Peaks {
    child: u16,
    parent: u16,
    anim: u16,
    memtree: u16,
    last_time: Option<u32>,
    entities: u32,
}

#[derive(Debug, Default)]
pub struct Sample {
    pub active: bool,
    pub time: u32,
    pub entities: u32,
    pub child: u16,
    pub child_max: u16,
    pub parent: u16,
    pub parent_max: u16,
    pub anim: u16,
    pub anim_max: u16,
    pub anim_active: usize,
    pub memtree: [u16; 16],
    pub memtree_max: u16,
    pub entities_active: usize,
    pub entities_free: usize,
    pub reset: Option<Reset>,
    pub frame_time: Option<u32>,
}

pub fn free_entities(bytes: &[u8], base: u32, first: u32, count: usize) -> Result<usize, String> {
    let mut ptr = first;
    let mut seen = HashSet::new();
    while ptr != 0 {
        let offset = ptr
            .checked_sub(base)
            .ok_or("free entity pointer precedes entity pool")? as usize;
        if !offset.is_multiple_of(ENTITY_SIZE) || offset / ENTITY_SIZE >= count {
            return Err(format!(
                "free entity pointer 0x{ptr:08X} is outside the entity pool"
            ));
        }
        if !seen.insert(ptr) {
            return Err("cycle in entity free list (map may be changing)".into());
        }
        ptr = u32_at(bytes, offset + 780);
    }
    Ok(seen.len()) // Includes the last element, whose nextFree is null.
}

pub fn sample(memory: &impl Memory, o: &Options, peaks: &mut Peaks) -> Result<Sample, String> {
    let state = memory.u32(SERVER_STATE)?;
    let level = memory.read(LEVEL, 0x79C)?;
    let entities = u32_at(&level, 12);
    let time = u32_at(&level, 0x798);
    let active = state == 2 && u32_at(&level, 0) != 0 && entities != 0;
    if !active
        || peaks.last_time.is_some_and(|previous| time < previous)
        || entities < peaks.entities
    {
        *peaks = Peaks::default();
    }
    peaks.last_time = Some(time);
    peaks.entities = entities;
    let mut s = Sample {
        active,
        time,
        entities,
        child: 0,
        child_max: 0,
        parent: 0,
        parent_max: 0,
        anim: 0,
        anim_max: 0,
        anim_active: 0,
        memtree: [0; 16],
        memtree_max: 0,
        entities_active: 0,
        entities_free: 0,
        reset: None,
        frame_time: None,
    };
    if o.frametime {
        s.frame_time = Some(memory.u32(FRAME_TIME)?);
    }
    if !active {
        return Ok(s);
    }
    if entities as usize > 1024 {
        return Err("unsupported entity layout or map changed while reading".into());
    }
    if o.child || o.parent {
        let variables = memory.read(SCRIPT_VARIABLES, 0x184)?;
        if o.child {
            s.child = u16_at(&memory.read(u32_at(&variables, 0x180), 16)?, 4);
            peaks.child = peaks.child.max(s.child);
            s.child_max = peaks.child;
        }
        if o.parent {
            s.parent = u16_at(&memory.read(u32_at(&variables, 0), 8)?, 2);
            peaks.parent = peaks.parent.max(s.parent);
            s.parent_max = peaks.parent;
        }
    }
    if o.anim {
        let anims = memory.read(ANIMS, ANIM_SIZE * ANIM_LIMIT)?;
        s.anim = u16_at(&anims, 10);
        s.anim_active = anims
            .as_chunks::<ANIM_SIZE>()
            .0
            .iter()
            .filter(|a| u16_at(*a, 18) != 0)
            .count();
        peaks.anim = peaks.anim.max(s.anim);
        s.anim_max = peaks.anim;
    }
    if o.memtree {
        let head = memory.u32(MEMTREE)?;
        let nodes = memory.read(head, 32)?;
        for (i, node) in s.memtree.iter_mut().enumerate() {
            *node = u16_at(&nodes, i * 2);
        }
        peaks.memtree = peaks.memtree.max(*s.memtree[..6].iter().max().unwrap());
        s.memtree_max = peaks.memtree;
    }
    if o.gspawn {
        if memory.u32(SERVER_ENTITY_SIZE)? as usize != ENTITY_SIZE {
            return Err("unsupported server entity size".into());
        }
        let base = u32_at(&level, 4);
        let count = (entities as usize).min(ENTITY_LIMIT);
        let pool = memory.read(base, ENTITY_SIZE * count)?;
        s.entities_active = pool
            .as_chunks::<ENTITY_SIZE>()
            .0
            .iter()
            .filter(|e| e[251] != 0)
            .count();
        s.entities_free = free_entities(&pool, base, u32_at(&level, 16), count)?;
    }
    if o.reset {
        s.reset = Some(server_snapshot(memory)?);
    }
    // Drop samples spanning a map transition rather than displaying mixed maps.
    if memory.u32(SERVER_STATE)? != state
        || memory.u32(LEVEL + 4)? != u32_at(&level, 4)
        || memory.u32(LEVEL + 0x798)? < time
    {
        return Err("map changed during sampling; reacquiring".into());
    }
    Ok(s)
}

impl Sample {
    pub fn render(&self, o: &Options) -> String {
        use std::fmt::Write;
        fn reading<T: std::fmt::Display>(value: Option<T>) -> String {
            value.map(|v| v.to_string()).unwrap_or_else(|| "--".into())
        }
        fn metric(out: &mut String, label: &str, value: impl std::fmt::Display, suffix: &str) {
            writeln!(out, "{label:<27}{value}{suffix}").unwrap();
        }
        let mut out = String::new();
        metric(
            &mut out,
            "level_time:",
            reading(self.active.then_some(self.time)),
            " ms",
        );
        if o.child {
            if !o.simple {
                metric(
                    &mut out,
                    "child_next:",
                    reading(self.active.then_some(self.child)),
                    "",
                );
            }
            metric(
                &mut out,
                "child_max:",
                reading(self.active.then_some(self.child_max)),
                " / 65536",
            );
        }
        if o.parent {
            if !o.simple {
                metric(
                    &mut out,
                    "parent_next:",
                    reading(self.active.then_some(self.parent)),
                    "",
                );
            }
            metric(
                &mut out,
                "parent_max:",
                reading(self.active.then_some(self.parent_max)),
                " / 32768",
            );
        }
        if o.anim {
            if !o.simple {
                metric(
                    &mut out,
                    "anims_active:",
                    reading(self.active.then_some(self.anim_active)),
                    "",
                );
                metric(
                    &mut out,
                    "anims_next:",
                    reading(self.active.then_some(self.anim)),
                    "",
                );
            }
            metric(
                &mut out,
                "anims_max:",
                reading(self.active.then_some(self.anim_max)),
                " / 4096",
            );
        }
        if o.memtree {
            for i in 0..if o.verbose { 16 } else { 1 } {
                metric(
                    &mut out,
                    &format!("memtree_next[{i:2}]:"),
                    reading(self.active.then_some(self.memtree[i])),
                    "",
                );
            }
            metric(
                &mut out,
                "memtree_max:",
                reading(self.active.then_some(self.memtree_max)),
                " / 65536",
            );
        }
        if o.gspawn {
            if !o.simple {
                metric(
                    &mut out,
                    "ents_in_use:",
                    reading(self.active.then_some(self.entities_active)),
                    "",
                );
            }
            metric(
                &mut out,
                "ents_free:",
                reading(self.active.then_some(self.entities_free)),
                "",
            );
            metric(
                &mut out,
                "num_entities:",
                reading(self.active.then_some(self.entities)),
                " / 1022",
            );
        }
        if o.reset {
            let reset = self.reset.as_ref().filter(|_| self.active);
            metric(
                &mut out,
                "snapshot_ents:",
                reading(reset.map(|r| r.snapshot_entities)),
                " (latest server snapshot)",
            );
            metric(
                &mut out,
                "svs.nextSnapshotEntities:",
                reset
                    .map(|r| format!("{} / {}", r.next, r.threshold))
                    .unwrap_or_else(|| "-- / --".into()),
                "",
            );
            metric(
                &mut out,
                "svs.numSnapshotEntities:",
                reading(reset.map(|r| r.capacity)),
                " (buffer capacity)",
            );
            metric(
                &mut out,
                "snapshot reset remaining:",
                reset
                    .map(|r| {
                        format!(
                            "{} ({:.6}% used)",
                            r.remaining,
                            f64::from(r.next) / f64::from(r.threshold) * 100.0
                        )
                    })
                    .unwrap_or_else(|| "--".into()),
                "",
            );
        }
        if o.frametime {
            metric(
                &mut out,
                "com_frameTime:",
                reading(self.frame_time),
                " / 2147483647",
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parent_limit_is_32768() {
        let options = Options::parse(["-p".into()]).unwrap().unwrap();
        let sample = Sample {
            active: true,
            parent_max: 20_000,
            ..Default::default()
        };
        assert!(sample.render(&options).contains("20000 / 32768"));
    }
    #[test]
    fn metric_rows_are_stable_while_waiting_live_and_at_reset_threshold() {
        for flags in [
            vec![],
            vec!["-r"],
            vec!["-sv"],
            vec!["-f"],
            vec!["-m", "-v"],
        ] {
            let options = Options::parse(flags.into_iter().map(str::to_owned))
                .unwrap()
                .unwrap();
            let waiting = Sample::default().render(&options);
            let live = Sample {
                active: true,
                reset: Some(Reset::new(1000, 6400).unwrap()),
                ..Default::default()
            }
            .render(&options);
            let limit = Sample {
                active: true,
                reset: Some(Reset::new(2_147_477_246, 6400).unwrap()),
                ..Default::default()
            }
            .render(&options);
            let labels = |text: &str| {
                text.lines()
                    .map(|line| line.split(':').next().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            assert_eq!(labels(&waiting), labels(&live));
            assert_eq!(labels(&live), labels(&limit));
            assert!(waiting.contains("--"));
            assert!(!limit.contains("THRESHOLD REACHED"));
        }
    }
    #[test]
    fn exact_server_reset_boundary() {
        let r = Reset::new(12_345, 25_600).unwrap();
        assert_eq!(r.threshold, 2_147_458_046);
        assert_eq!(r.remaining, r.threshold - 12_345);
        assert_eq!(
            Reset::new(r.threshold - 1, r.capacity).unwrap().remaining,
            1
        );
        assert_eq!(Reset::new(r.threshold, r.capacity).unwrap().remaining, 0);
        assert_eq!(
            Reset::new(r.threshold + 1, r.capacity).unwrap().remaining,
            0
        );
        assert!(Reset::new(0, 0).is_err());
        assert!(Reset::new(u32::MAX, 100).is_err());
    }
    #[test]
    fn free_list_includes_tail_and_rejects_cycles_and_bad_pointers() {
        let mut pool = vec![0; ENTITY_SIZE * 3];
        let base = 0x1000;
        pool[780..784].copy_from_slice(&(base + ENTITY_SIZE as u32).to_le_bytes());
        assert_eq!(free_entities(&pool, base, base, 3).unwrap(), 2);
        assert_eq!(free_entities(&pool, base, 0, 3).unwrap(), 0);
        assert!(free_entities(&pool, base, base + 1, 3).is_err());
        assert!(free_entities(&pool, base, base + ENTITY_SIZE as u32 * 3, 3).is_err());
        pool[ENTITY_SIZE + 780..ENTITY_SIZE + 784].copy_from_slice(&base.to_le_bytes());
        assert!(free_entities(&pool, base, base, 3).is_err());
    }
    struct Fake(std::collections::BTreeMap<u32, Vec<u8>>);
    impl Memory for Fake {
        fn read(&self, address: u32, size: usize) -> Result<Vec<u8>, String> {
            self.0
                .get(&address)
                .filter(|b| b.len() == size)
                .cloned()
                .ok_or_else(|| format!("unexpected read 0x{address:X}/{size}"))
        }
    }
    fn snapshot_memory(next: u32, count: u32, capacity: u32, index: u32) -> Fake {
        let mut m = Fake(Default::default());
        let mut block = vec![0; (NEXT_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize + 4];
        let frame = ((index as usize + 15) % 16) * SERVER_SNAPSHOT_SIZE;
        for (offset, value) in [
            ((NEXT_SERVER_SNAPSHOT - SERVER_SNAPSHOTS) as usize, index),
            (
                (NUM_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize,
                capacity,
            ),
            ((NEXT_SNAPSHOT_ENTITIES - SERVER_SNAPSHOTS) as usize, next),
            (frame + 72, count),
            (frame + 84, next.saturating_sub(count)),
        ] {
            block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        m.0.insert(SERVER_SNAPSHOTS, block);
        m.0.insert(NEXT_SERVER_SNAPSHOT, index.to_le_bytes().to_vec());
        m
    }
    #[test]
    fn snapshot_count_is_independent_of_cumulative_total_and_poll_interval() {
        for index in [0, 1, 15] {
            let m = snapshot_memory(355_937, 272, 6400, index);
            let r = server_snapshot(&m).unwrap();
            assert_eq!(r.snapshot_entities, 272);
            assert_eq!(r.next, 355_937);
            assert_eq!(server_snapshot(&m).unwrap(), r);
            assert_eq!(r.threshold, 2_147_477_246);
        }
        assert_eq!(
            server_snapshot(&snapshot_memory(0, 0, 6400, 0))
                .unwrap()
                .snapshot_entities,
            0
        );
    }
    #[test]
    fn rejects_invalid_or_in_progress_snapshot_records() {
        assert!(server_snapshot(&snapshot_memory(10_000, 1025, 6400, 1)).is_err());
        assert!(server_snapshot(&snapshot_memory(10_000, 272, 6400, 16)).is_err());
        let mut m = snapshot_memory(10_000, 272, 6400, 1);
        let block = m.0.get_mut(&SERVER_SNAPSHOTS).unwrap();
        block[72..76].copy_from_slice(&271u32.to_le_bytes());
        assert!(server_snapshot(&m).is_err());
        let mut m = snapshot_memory(10_000, 272, 6400, 1);
        m.0.insert(NEXT_SERVER_SNAPSHOT, 2u32.to_le_bytes().to_vec());
        assert!(server_snapshot(&m).is_err());
    }
    #[test]
    fn reset_only_reads_server_globals_and_refreshes_map_without_gspawn() {
        let o = Options::parse(["-r".into()]).unwrap().unwrap();
        let mut m = snapshot_memory(987_654, 272, 25_600, 1);
        let mut level = vec![0; 0x79C];
        for (offset, value) in [
            (0, 0x1000u32),
            (4, 0x2000),
            (8, ENTITY_SIZE as u32),
            (12, 50),
            (0x798, 1000),
        ] {
            level[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        m.0.insert(SERVER_STATE, 2u32.to_le_bytes().to_vec());
        m.0.insert(LEVEL, level.clone());
        m.0.insert(LEVEL + 4, 0x2000u32.to_le_bytes().to_vec());
        m.0.insert(LEVEL + 0x798, 1000u32.to_le_bytes().to_vec());
        let mut peaks = Peaks {
            child: 900,
            ..Default::default()
        };
        assert_eq!(
            sample(&m, &o, &mut peaks).unwrap().reset.unwrap().next,
            987654
        );
        level[0x798..0x79C].copy_from_slice(&50u32.to_le_bytes());
        m.0.insert(LEVEL, level);
        m.0.insert(LEVEL + 0x798, 50u32.to_le_bytes().to_vec());
        sample(&m, &o, &mut peaks).unwrap();
        assert_eq!(peaks.child, 0);
        m.0.insert(SERVER_STATE, 0u32.to_le_bytes().to_vec());
        let s = sample(&m, &o, &mut peaks).unwrap();
        assert!(!s.active);
        assert!(s.reset.is_none());
    }
}
