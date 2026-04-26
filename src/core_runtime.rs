pub const PAGE_SIZE: i64 = 4096;
const PAGE_MASK: i64 = PAGE_SIZE - 1;
const PT_SHIFT: u32 = 12;
const PD_SHIFT: u32 = 21;
const PDPT_SHIFT: u32 = 30;
const PML4_SHIFT: u32 = 39;

pub fn word_bits() -> i64 {
    (std::mem::size_of::<usize>() * 8) as i64
}

pub fn word_bytes() -> i64 {
    std::mem::size_of::<usize>() as i64
}

pub fn page_size() -> i64 {
    PAGE_SIZE
}

pub fn page_align_down(addr: i64) -> i64 {
    addr & !PAGE_MASK
}

pub fn page_align_up(addr: i64) -> i64 {
    (addr + PAGE_MASK) & !PAGE_MASK
}

pub fn page_offset(addr: i64) -> i64 {
    addr & PAGE_MASK
}

pub fn page_index(addr: i64) -> i64 {
    addr >> PT_SHIFT
}

pub fn page_count(bytes: i64) -> i64 {
    if bytes <= 0 {
        0
    } else {
        (bytes + PAGE_MASK) >> PT_SHIFT
    }
}

fn page_level_index(addr: i64, shift: u32) -> i64 {
    (addr >> shift) & 0x1ff
}

pub fn pt_index(addr: i64) -> i64 {
    page_level_index(addr, PT_SHIFT)
}

pub fn pd_index(addr: i64) -> i64 {
    page_level_index(addr, PD_SHIFT)
}

pub fn pdpt_index(addr: i64) -> i64 {
    page_level_index(addr, PDPT_SHIFT)
}

pub fn pml4_index(addr: i64) -> i64 {
    page_level_index(addr, PML4_SHIFT)
}
