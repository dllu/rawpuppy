//! Distinct filenames and namespace keep Darktable's sidecars independent.
use crate::edits::Edits;
use anyhow::{Context, Result, bail, ensure};
use quick_xml::{NsReader, events::Event, name::ResolveResult};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const NAMESPACE: &str = "https://rawpuppy.org/ns/1.0/";

pub fn path_for(original: &Path) -> PathBuf {
    let mut name = original.as_os_str().to_os_string();
    name.push(".rawpuppy.xmp");
    PathBuf::from(name)
}

pub fn load(path: &Path) -> Result<Edits> {
    let xml = fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let mut reader = NsReader::from_str(&xml);
    let mut recipe = String::new();
    let mut in_recipe = false;
    let mut found = false;
    loop {
        let (namespace, event) = reader.read_resolved_event()?;
        match event {
            Event::Start(e)
                if e.local_name().as_ref() == b"Recipe"
                    && matches!(namespace,ResolveResult::Bound(ns) if ns.as_ref() == NAMESPACE.as_bytes()) =>
            {
                ensure!(!found, "Sidecar contains multiple Rawpuppy recipes");
                in_recipe = true;
                found = true;
            }
            Event::Start(_) if in_recipe => bail!("Unexpected XML inside Rawpuppy recipe"),
            Event::Text(e) if in_recipe => recipe.push_str(&e.xml10_content()?),
            Event::CData(e) if in_recipe => recipe.push_str(&e.xml10_content()?),
            Event::GeneralRef(e) if in_recipe => {
                let reference = format!("&{};", e.decode()?);
                recipe.push_str(&quick_xml::escape::unescape(&reference)?);
            }
            Event::End(e) if in_recipe && e.local_name().as_ref() == b"Recipe" => in_recipe = false,
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(
        found && !in_recipe,
        "No complete Rawpuppy recipe found in sidecar"
    );
    let edits: Edits = serde_json::from_str(&recipe).context("Decoding XMP edit recipe")?;
    edits.validate()?;
    Ok(edits)
}

pub fn load_for(original: &Path) -> Result<Edits> {
    let path = path_for(original);
    match fs::metadata(&path) {
        Ok(_) => load(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Edits::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save(path: &Path, edits: &Edits) -> Result<()> {
    edits.validate()?;
    let recipe = serde_json::to_string_pretty(edits)?;
    let recipe = quick_xml::escape::escape(&recipe);
    let xml = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
        <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
        <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
        <rdf:Description rdf:about=\"\" xmlns:rp=\"{NAMESPACE}\">\n\
        <rp:Recipe>{recipe}</rp:Recipe>\n\
        </rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(xml.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .with_context(|| format!("Saving {}", path.display()))?;
    Ok(())
}
