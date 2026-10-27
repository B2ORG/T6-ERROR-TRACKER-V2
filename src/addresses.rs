//! T6 Zombies win32 addresses, shared by Redacted v41 and Plutonium r5359.
//! Remote pointers are always u32, even when the tracker itself is 64-bit.
pub const LEVEL: u32 = 0x0233_8780;
pub const ANIMS: u32 = 0x02B3_F140;
pub const STRINGS: u32 = 0x02BF_3280;
pub const SCRIPT_VARIABLES: u32 = 0x02DE_A400;
pub const MEMTREE: u32 = 0x02BF_3180;
pub const FRAME_TIME: u32 = 0x025B_6130;
pub const SERVER_STATE: u32 = 0x028B_6888;
pub const SERVER_ENTITY_SIZE: u32 = 0x028C_069C;
pub const SERVER_SNAPSHOTS: u32 = 0x028E_C324;
pub const NEXT_SERVER_SNAPSHOT: u32 = 0x028E_C9A4;
pub const SERVER_SNAPSHOT_SIZE: usize = 100;
pub const SERVER_SNAPSHOT_COUNT: usize = 16;
pub const NUM_SNAPSHOT_ENTITIES: u32 = 0x028E_C9D8;
pub const NEXT_SNAPSHOT_ENTITIES: u32 = 0x028E_C9E8;
pub const CHECK_OVERFLOW: u32 = 0x0089_23E0;
pub const ENTITY_SIZE: usize = 0x31C;
pub const ENTITY_LIMIT: usize = 1022;
pub const ANIM_SIZE: usize = 64;
pub const ANIM_LIMIT: usize = 4096;
