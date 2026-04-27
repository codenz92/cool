use object::read::archive::ArchiveFile;
use object::{Object, ObjectSection, ObjectSymbol};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct LayoutReport {
    pub path: String,
    pub file_kind: String,
    pub format: String,
    pub architecture: String,
    pub endianness: String,
    pub entry: Option<u64>,
    pub sections: Vec<LayoutSection>,
    pub symbols: Vec<LayoutSymbol>,
    pub members: Vec<LayoutMember>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayoutSection {
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub align: u64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayoutSymbol {
    pub name: String,
    pub address: u64,
    pub size: u64,
    pub kind: String,
    pub section: Option<String>,
    pub defined: bool,
    pub global: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayoutMember {
    pub name: String,
    pub format: String,
    pub architecture: String,
    pub sections: Vec<LayoutSection>,
    pub symbols: Vec<LayoutSymbol>,
}

pub fn inspect_path(path: &Path) -> Result<LayoutReport, String> {
    let data = fs::read(path).map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
    let file_kind = object::FileKind::parse(&*data).map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
    match file_kind {
        object::FileKind::Archive => inspect_archive(path, &data),
        _ => {
            let file = object::File::parse(&*data).map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
            let (sections, symbols) = collect_object_details(&file);
            Ok(LayoutReport {
                path: path.display().to_string(),
                file_kind: format!("{file_kind:?}").to_lowercase(),
                format: format!("{:?}", file.format()).to_lowercase(),
                architecture: format!("{:?}", file.architecture()).to_lowercase(),
                endianness: format!("{:?}", file.endianness()).to_lowercase(),
                entry: Some(file.entry()).filter(|entry| *entry != 0),
                sections,
                symbols,
                members: Vec::new(),
            })
        }
    }
}

fn inspect_archive(path: &Path, data: &[u8]) -> Result<LayoutReport, String> {
    let archive = ArchiveFile::parse(data).map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
    let mut members = Vec::new();
    for member in archive.members() {
        let member = member.map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
        let name = String::from_utf8_lossy(member.name()).to_string();
        let member_data = member
            .data(data)
            .map_err(|e| format!("cool layout: {}: {e}", path.display()))?;
        if member_data.is_empty() {
            continue;
        }
        let Ok(file) = object::File::parse(member_data) else {
            continue;
        };
        let (sections, symbols) = collect_object_details(&file);
        members.push(LayoutMember {
            name,
            format: format!("{:?}", file.format()).to_lowercase(),
            architecture: format!("{:?}", file.architecture()).to_lowercase(),
            sections,
            symbols,
        });
    }
    Ok(LayoutReport {
        path: path.display().to_string(),
        file_kind: "archive".to_string(),
        format: "archive".to_string(),
        architecture: "multi".to_string(),
        endianness: "mixed".to_string(),
        entry: None,
        sections: Vec::new(),
        symbols: Vec::new(),
        members,
    })
}

fn collect_object_details(file: &object::File<'_>) -> (Vec<LayoutSection>, Vec<LayoutSymbol>) {
    let mut sections = file
        .sections()
        .filter_map(|section| {
            let name = section.name().ok()?.to_string();
            Some(LayoutSection {
                name,
                address: section.address(),
                size: section.size(),
                align: section.align(),
                kind: format!("{:?}", section.kind()).to_lowercase(),
            })
        })
        .collect::<Vec<_>>();
    sections.sort_by(|left, right| {
        left.address
            .cmp(&right.address)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut symbols = file
        .symbols()
        .filter_map(|symbol| {
            let name = symbol.name().ok()?.to_string();
            if name.is_empty() {
                return None;
            }
            Some(LayoutSymbol {
                name,
                address: symbol.address(),
                size: symbol.size(),
                kind: format!("{:?}", symbol.kind()).to_lowercase(),
                section: symbol
                    .section_index()
                    .and_then(|index| file.section_by_index(index).ok())
                    .and_then(|section| section.name().ok().map(str::to_string)),
                defined: !symbol.is_undefined(),
                global: symbol.is_global(),
            })
        })
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| {
        left.address
            .cmp(&right.address)
            .then_with(|| left.name.cmp(&right.name))
    });
    (sections, symbols)
}

pub fn render_text(report: &LayoutReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("path: {}\n", report.path));
    out.push_str(&format!("kind: {}\n", report.file_kind));
    out.push_str(&format!("format: {}\n", report.format));
    out.push_str(&format!("arch: {}\n", report.architecture));
    out.push_str(&format!("endianness: {}\n", report.endianness));
    if let Some(entry) = report.entry {
        out.push_str(&format!("entry: 0x{entry:x}\n"));
    }

    if !report.sections.is_empty() {
        out.push_str("\nsections:\n");
        for section in &report.sections {
            out.push_str(&format!(
                "  {:<18} addr=0x{:x} size=0x{:x} align={} kind={}\n",
                section.name, section.address, section.size, section.align, section.kind
            ));
        }
    }

    if !report.symbols.is_empty() {
        out.push_str("\nsymbols:\n");
        for symbol in &report.symbols {
            let section = symbol.section.as_deref().unwrap_or("<none>");
            let defined = if symbol.defined { "defined" } else { "undefined" };
            let visibility = if symbol.global { "global" } else { "local" };
            out.push_str(&format!(
                "  {:<24} addr=0x{:x} size=0x{:x} {} {} section={}\n",
                symbol.name, symbol.address, symbol.size, defined, visibility, section
            ));
        }
    }

    if !report.members.is_empty() {
        out.push_str("\nmembers:\n");
        for member in &report.members {
            out.push_str(&format!("  {}\n", member.name));
            out.push_str(&format!("    format: {}\n", member.format));
            out.push_str(&format!("    arch: {}\n", member.architecture));
            for section in &member.sections {
                out.push_str(&format!(
                    "    section {:<16} addr=0x{:x} size=0x{:x} align={} kind={}\n",
                    section.name, section.address, section.size, section.align, section.kind
                ));
            }
        }
    }

    out
}
