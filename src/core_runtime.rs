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

pub fn addr(value: i64) -> i64 {
    wrap_addr(value as i128)
}

pub fn addr_add(base: i64, offset: i64) -> i64 {
    wrap_addr(base as i128 + offset as i128)
}

pub fn addr_sub(base: i64, offset: i64) -> i64 {
    wrap_addr(base as i128 - offset as i128)
}

pub fn addr_diff(left: i64, right: i64) -> i64 {
    left.wrapping_sub(right)
}

pub fn addr_align_down(value: i64, align: i64) -> Result<i64, String> {
    let mask = align_mask(align)?;
    Ok(addr(value) & !mask)
}

pub fn addr_align_up(value: i64, align: i64) -> Result<i64, String> {
    let mask = align_mask(align)?;
    Ok(addr(addr(value).wrapping_add(mask)) & !mask)
}

pub fn addr_is_aligned(value: i64, align: i64) -> Result<bool, String> {
    let mask = align_mask(align)?;
    Ok((addr(value) & mask) == 0)
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

pub fn string_len(text: &str) -> i64 {
    text.chars().count() as i64
}

pub fn string_repeat(text: &str, count: i64) -> String {
    text.repeat(count.max(0) as usize)
}

pub fn format_hex(value: i64) -> String {
    format!("0x{:x}", value as u64)
}

pub fn format_bin(value: i64) -> String {
    format!("0b{:b}", value as u64)
}

pub fn format_ptr(value: i64) -> String {
    let digits = (word_bytes() as usize) * 2;
    format!("0x{:0digits$x}", addr(value) as u64, digits = digits)
}

fn align_mask(align: i64) -> Result<i64, String> {
    if align <= 0 {
        return Err("alignment must be positive".to_string());
    }
    if (align & (align - 1)) != 0 {
        return Err("alignment must be a power of two".to_string());
    }
    Ok(align - 1)
}

fn wrap_addr(value: i128) -> i64 {
    let bits = word_bits() as u32;
    if bits >= 64 {
        value as i64
    } else {
        let mask = (1i128 << bits) - 1;
        (value & mask) as i64
    }
}
