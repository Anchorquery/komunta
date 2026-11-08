//! The team's documents - a script, the captions, notes - read as lines,
//! and what each is for by its name. Plain text and Markdown are read as
//! UTF-8; a Word file by the paragraphs of its body, as the engine's
//! `python-docx` read them.

use std::io::{self, Read};
use std::path::Path;

use crate::inventario::Rol;

/// What a document named `nombre` is for: a script (`guion`, `gidoi`,
/// `script`), captions (`subt`, `azpititul`) or notes.
pub fn rol(nombre: &str) -> Rol {
    let nombre = nombre.to_lowercase();
    let tiene = |trozos: &[&str]| trozos.iter().any(|t| nombre.contains(t));
    if tiene(&["gidoi", "guion", "guión", "script"]) {
        Rol::Guion
    } else if tiene(&["subt", "azpititul"]) {
        Rol::Subtitulos
    } else {
        Rol::Notas
    }
}

/// The document's lines, trimmed, without the empty ones.
pub fn lineas(ruta: &Path) -> io::Result<Vec<String>> {
    let docx = ruta
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("docx"));
    let crudas = if docx {
        parrafos_docx(&std::fs::read(ruta)?)?
    } else {
        lineas_de_texto(&String::from_utf8_lossy(&std::fs::read(ruta)?))
    };
    Ok(crudas
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Text split where Python's `str.splitlines` splits it; the empty lines
/// that leaves are dropped after.
fn lineas_de_texto(texto: &str) -> Vec<String> {
    let corte = |c: char| {
        matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    };
    texto
        .replace("\r\n", "\n")
        .split(corte)
        .map(str::to_owned)
        .collect()
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// The text of each paragraph straight in a Word file's body - not the
/// tables', the headers' or the text boxes' - as `python-docx` gives it:
/// the runs, also inside links, with tabs and line breaks.
fn parrafos_docx(bytes: &[u8]) -> io::Result<Vec<String>> {
    let mut zip = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(io::Error::other)?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(io::Error::other)?
        .read_to_string(&mut xml)?;
    let documento = roxmltree::Document::parse(&xml)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let cuerpo = hijos(documento.root_element(), "body")
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a Word file with no body"))?;
    Ok(hijos(cuerpo, "p").map(texto_parrafo).collect())
}

fn texto_parrafo(parrafo: roxmltree::Node) -> String {
    let mut texto = String::new();
    for hijo in parrafo
        .children()
        .filter(|n| es(*n, "r") || es(*n, "hyperlink"))
    {
        if es(hijo, "r") {
            texto_tramo(hijo, &mut texto);
        } else {
            hijos(hijo, "r").for_each(|tramo| texto_tramo(tramo, &mut texto));
        }
    }
    texto
}

fn texto_tramo(tramo: roxmltree::Node, texto: &mut String) {
    for pieza in tramo.children().filter(roxmltree::Node::is_element) {
        match pieza.tag_name().name() {
            "t" => texto.push_str(pieza.text().unwrap_or_default()),
            "tab" | "ptab" => texto.push('\t'),
            "cr" => texto.push('\n'),
            // A page or column break is no text; a plain one is a new line.
            "br" if matches!(pieza.attribute((W, "type")), None | Some("textWrapping")) => {
                texto.push('\n');
            }
            "noBreakHyphen" => texto.push('-'),
            _ => {}
        }
    }
}

fn hijos<'a, 'i>(
    nodo: roxmltree::Node<'a, 'i>,
    nombre: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    nodo.children().filter(move |n| es(*n, nombre))
}

fn es(nodo: roxmltree::Node, nombre: &str) -> bool {
    nodo.is_element() && nodo.tag_name().namespace() == Some(W) && nodo.tag_name().name() == nombre
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn a_document_s_role_is_in_its_name() {
        assert_eq!(rol("Gidoia_azkena.docx"), Rol::Guion);
        assert_eq!(rol("GUIÓN.txt"), Rol::Guion);
        assert_eq!(rol("script final.md"), Rol::Guion);
        assert_eq!(rol("Subtitulos.txt"), Rol::Subtitulos);
        assert_eq!(rol("azpitituluak.txt"), Rol::Subtitulos);
        assert_eq!(rol("notas.txt"), Rol::Notas);
    }

    #[test]
    fn text_is_split_as_python_splits_it() {
        assert_eq!(
            lineas_de_texto("a\r\nb\rc\u{2028}d\n"),
            ["a", "b", "c", "d", ""]
        );
    }

    fn carpeta(nombre: &str) -> std::path::PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-doc-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    #[test]
    fn a_text_file_keeps_its_non_empty_lines_trimmed() {
        let ruta = carpeta("txt").join("guion.txt");
        std::fs::write(&ruta, "  Kaixo!  \n\n\tBuenas\r\n   \n").unwrap();
        assert_eq!(lineas(&ruta).unwrap(), ["Kaixo!", "Buenas"]);
    }

    fn docx(cuerpo: &str) -> Vec<u8> {
        let mut bytes = io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(
            zip,
            r#"<?xml version="1.0"?><w:document xmlns:w="{W}"><w:body>{cuerpo}</w:body></w:document>"#
        )
        .unwrap();
        zip.finish().unwrap();
        bytes.into_inner()
    }

    #[test]
    fn a_word_file_is_its_body_s_paragraphs() {
        let cuerpo = r#"
            <w:p><w:r><w:t xml:space="preserve">Hola, </w:t></w:r><w:r><w:t>mundo</w:t></w:r></w:p>
            <w:p/>
            <w:p><w:hyperlink><w:r><w:t>enlace</w:t></w:r></w:hyperlink><w:r><w:tab/><w:t>x</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>en la tabla</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            <w:p><w:r><w:t>uno</w:t><w:br/><w:t>dos</w:t><w:br w:type="page"/></w:r></w:p>"#;
        let ruta = carpeta("docx").join("guion.docx");
        std::fs::write(&ruta, docx(cuerpo)).unwrap();
        assert_eq!(
            lineas(&ruta).unwrap(),
            ["Hola, mundo", "enlace\tx", "uno\ndos"]
        );
    }

    #[test]
    fn a_broken_word_file_is_an_error() {
        let ruta = carpeta("roto").join("~$guion.docx");
        std::fs::write(&ruta, b"not a zip").unwrap();
        assert!(lineas(&ruta).is_err());
    }
}
