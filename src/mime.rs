//! Content type guessing compatible with the default tables of Python's
//! `mimetypes` module, which the AWS CLI uses to set the content type.
//! The tables are generated from Python's `mimetypes._types_map_default`,
//! `_suffix_map_default` and `_encodings_map_default`, and the types in the
//! known files of the system are added like `mimetypes.init`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The files of the system which Python's `mimetypes` reads in order.
const KNOWN_FILES: &[&str] = &[
    "/etc/mime.types",
    "/etc/httpd/mime.types",
    "/etc/httpd/conf/mime.types",
    "/etc/apache/mime.types",
    "/etc/apache2/mime.types",
    "/usr/local/etc/httpd/conf/mime.types",
    "/usr/local/lib/netscape/mime.types",
    "/usr/local/etc/httpd/conf/mime.types",
    "/usr/local/etc/mime.types",
];

/// Extensions (lowercase, with the leading dot) and their content types.
const TYPES: &[(&str, &str)] = &[
    (".3g2", "audio/3gpp2"),
    (".3gp", "audio/3gpp"),
    (".3gpp", "audio/3gpp"),
    (".3gpp2", "audio/3gpp2"),
    (".7z", "application/x-7z-compressed"),
    (".a", "application/octet-stream"),
    (".aac", "audio/aac"),
    (".adts", "audio/aac"),
    (".ai", "application/pdf"),
    (".aif", "audio/x-aiff"),
    (".aifc", "audio/x-aiff"),
    (".aiff", "audio/x-aiff"),
    (".ass", "audio/aac"),
    (".au", "audio/basic"),
    (".avi", "video/vnd.avi"),
    (".avif", "image/avif"),
    (".bat", "text/plain"),
    (".bcpio", "application/x-bcpio"),
    (".bin", "application/octet-stream"),
    (".bmp", "image/bmp"),
    (".c", "text/plain"),
    (".cab", "application/vnd.ms-cab-compressed"),
    (".cdf", "application/x-netcdf"),
    (".chm", "application/vnd.ms-htmlhelp"),
    (".cjs", "application/node"),
    (".cpio", "application/x-cpio"),
    (".csh", "application/x-csh"),
    (".css", "text/css"),
    (".csv", "text/csv"),
    (".dcm", "application/dicom"),
    (".deb", "application/x-debian-package"),
    (".dll", "application/vnd.microsoft.portable-executable"),
    (".doc", "application/msword"),
    (
        ".docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (".dot", "application/msword"),
    (".dvi", "application/x-dvi"),
    (".efi", "application/efi"),
    (".emf", "image/emf"),
    (".eml", "message/rfc822"),
    (".eot", "application/vnd.ms-fontobject"),
    (".eps", "application/postscript"),
    (".epub", "application/epub+zip"),
    (".etx", "text/x-setext"),
    (".exe", "application/vnd.microsoft.portable-executable"),
    (".fits", "image/fits"),
    (".flac", "audio/flac"),
    (".g3", "image/g3fax"),
    (".gif", "image/gif"),
    (".glb", "model/gltf-binary"),
    (".gltf", "model/gltf+json"),
    (".gtar", "application/x-gtar"),
    (".gz", "application/gzip"),
    (".h", "text/plain"),
    (".h5", "application/x-hdf5"),
    (".hdf", "application/x-hdf"),
    (".heic", "image/heic"),
    (".heif", "image/heif"),
    (".hjif", "haptics/hjif"),
    (".hmpg", "haptics/hmpg"),
    (".htm", "text/html"),
    (".html", "text/html"),
    (".ico", "image/vnd.microsoft.icon"),
    (".ief", "image/ief"),
    (".ivs", "haptics/ivs"),
    (".jp2", "image/jp2"),
    (".jpe", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".jpg", "image/jpeg"),
    (".jpm", "image/jpm"),
    (".jpx", "image/jpx"),
    (".js", "text/javascript"),
    (".json", "application/json"),
    (".jxl", "image/jxl"),
    (".ksh", "text/plain"),
    (".latex", "application/x-latex"),
    (".loas", "audio/aac"),
    (".m1v", "video/mpeg"),
    (".m3u", "application/vnd.apple.mpegurl"),
    (".m3u8", "application/vnd.apple.mpegurl"),
    (".m4a", "audio/mp4"),
    (".m4v", "video/x-m4v"),
    (".man", "application/x-troff-man"),
    (".markdown", "text/markdown"),
    (".md", "text/markdown"),
    (".me", "application/x-troff-me"),
    (".mht", "message/rfc822"),
    (".mhtml", "message/rfc822"),
    (".mif", "application/x-mif"),
    (".mjs", "text/javascript"),
    (".mk3d", "video/matroska-3d"),
    (".mka", "audio/matroska"),
    (".mkv", "video/matroska"),
    (".mov", "video/quicktime"),
    (".movie", "video/x-sgi-movie"),
    (".mp2", "audio/mpeg"),
    (".mp3", "audio/mpeg"),
    (".mp4", "video/mp4"),
    (".mpa", "video/mpeg"),
    (".mpe", "video/mpeg"),
    (".mpeg", "video/mpeg"),
    (".mpg", "video/mpeg"),
    (".ms", "application/x-troff-ms"),
    (".n3", "text/n3"),
    (".nc", "application/x-netcdf"),
    (".nq", "application/n-quads"),
    (".nt", "application/n-triples"),
    (".nws", "message/rfc822"),
    (".o", "application/octet-stream"),
    (".obj", "application/octet-stream"),
    (".oda", "application/oda"),
    (".odg", "application/vnd.oasis.opendocument.graphics"),
    (".odp", "application/vnd.oasis.opendocument.presentation"),
    (".ods", "application/vnd.oasis.opendocument.spreadsheet"),
    (".odt", "application/vnd.oasis.opendocument.text"),
    (".ogg", "audio/ogg"),
    (".ogv", "video/ogg"),
    (".ogx", "application/ogg"),
    (".opus", "audio/opus"),
    (".otf", "font/otf"),
    (".p12", "application/x-pkcs12"),
    (".p7c", "application/pkcs7-mime"),
    (".pbm", "image/x-portable-bitmap"),
    (".pdf", "application/pdf"),
    (".pfx", "application/x-pkcs12"),
    (".pgm", "image/x-portable-graymap"),
    (".php", "application/x-httpd-php"),
    (".pl", "text/plain"),
    (".png", "image/png"),
    (".pnm", "image/x-portable-anymap"),
    (".pot", "application/vnd.ms-powerpoint"),
    (".ppa", "application/vnd.ms-powerpoint"),
    (".ppm", "image/x-portable-pixmap"),
    (".pps", "application/vnd.ms-powerpoint"),
    (".ppt", "application/vnd.ms-powerpoint"),
    (
        ".pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    (".ps", "application/postscript"),
    (".pwz", "application/vnd.ms-powerpoint"),
    (".py", "text/x-python"),
    (".pyc", "application/x-python-code"),
    (".pyo", "application/x-python-code"),
    (".qt", "video/quicktime"),
    (".ra", "audio/x-pn-realaudio"),
    (".ram", "application/x-pn-realaudio"),
    (".rar", "application/vnd.rar"),
    (".ras", "image/x-cmu-raster"),
    (".rdf", "application/xml"),
    (".rgb", "image/x-rgb"),
    (".roff", "application/x-troff"),
    (".rpm", "application/x-rpm"),
    (".rst", "text/x-rst"),
    (".rtf", "application/rtf"),
    (".rtx", "text/richtext"),
    (".sgm", "text/x-sgml"),
    (".sgml", "text/x-sgml"),
    (".sh", "application/x-sh"),
    (".shar", "application/x-shar"),
    (".snd", "audio/basic"),
    (".so", "application/octet-stream"),
    (".sql", "application/sql"),
    (".sqlite", "application/vnd.sqlite3"),
    (".sqlite3", "application/vnd.sqlite3"),
    (".src", "application/x-wais-source"),
    (".srt", "text/plain"),
    (".stl", "model/stl"),
    (".sv4cpio", "application/x-sv4cpio"),
    (".sv4crc", "application/x-sv4crc"),
    (".svg", "image/svg+xml"),
    (".swf", "application/x-shockwave-flash"),
    (".t", "application/x-troff"),
    (".t38", "image/t38"),
    (".tar", "application/x-tar"),
    (".tcl", "application/x-tcl"),
    (".tex", "application/x-tex"),
    (".texi", "application/texinfo"),
    (".texinfo", "application/texinfo"),
    (".tfx", "image/tiff-fx"),
    (".thmx", "application/vnd.ms-officetheme"),
    (".tif", "image/tiff"),
    (".tiff", "image/tiff"),
    (".toml", "application/toml"),
    (".tr", "application/x-troff"),
    (".trig", "application/trig"),
    (".tsv", "text/tab-separated-values"),
    (".ttc", "font/collection"),
    (".ttf", "font/ttf"),
    (".txt", "text/plain"),
    (".ustar", "application/x-ustar"),
    (".vcf", "text/x-vcard"),
    (".vtt", "text/vtt"),
    (".wasm", "application/wasm"),
    (".wav", "audio/vnd.wave"),
    (".weba", "audio/webm"),
    (".webm", "video/webm"),
    (".webmanifest", "application/manifest+json"),
    (".webp", "image/webp"),
    (".wiz", "application/msword"),
    (".wmf", "image/wmf"),
    (".wmv", "video/x-ms-wmv"),
    (".woff", "font/woff"),
    (".woff2", "font/woff2"),
    (".wsdl", "application/xml"),
    (".xbm", "image/x-xbitmap"),
    (".xlb", "application/vnd.ms-excel"),
    (".xls", "application/vnd.ms-excel"),
    (
        ".xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (".xml", "text/xml"),
    (".xpdl", "application/xml"),
    (".xpm", "image/x-xpixmap"),
    (".xsl", "application/xml"),
    (".xwd", "image/x-xwindowdump"),
    (".yaml", "application/yaml"),
    (".yml", "application/yaml"),
    (".zip", "application/zip"),
];

/// Extensions that are aliases of compound extensions.
const SUFFIXES: &[(&str, &str)] = &[
    (".svgz", ".svg.gz"),
    (".taz", ".tar.gz"),
    (".tbz2", ".tar.bz2"),
    (".tgz", ".tar.gz"),
    (".txz", ".tar.xz"),
    (".tz", ".tar.gz"),
];

/// Extensions of encodings, which are case-sensitive.
const ENCODINGS: &[&str] = &[".Z", ".br", ".bz2", ".gz", ".xz"];

/// Guesses the content type of the path, in the same way as
/// `mimetypes.guess_type` of Python.
pub fn guess_content_type(path: &str) -> Option<&'static str> {
    let mut path = path.to_owned();
    let mut index = extension_index(&path);
    while let Some(start) = index
        && let Some((_, suffix)) = SUFFIXES
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&path[start..]))
    {
        path.truncate(start);
        path.push_str(suffix);
        index = extension_index(&path);
    }
    let mut start = index?;
    if ENCODINGS.contains(&&path[start..]) {
        path.truncate(start);
        start = extension_index(&path)?;
    }
    types_map()
        .get(&path[start..].to_lowercase())
        .map(String::as_str)
}

/// Returns the types of the extensions, where the types in the known files
/// override the defaults.
fn types_map() -> &'static HashMap<String, String> {
    static TYPES_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    TYPES_MAP.get_or_init(|| {
        let mut map: HashMap<String, String> = TYPES
            .iter()
            .map(|(ext, content_type)| ((*ext).to_owned(), (*content_type).to_owned()))
            .collect();
        for file in KNOWN_FILES {
            if let Ok(content) = std::fs::read_to_string(file) {
                read_mime_types(&content, &mut map);
            }
        }
        map
    })
}

/// Reads the types in the format of `mime.types`, where each line is a type
/// followed by the extensions without the leading dots.
fn read_mime_types(content: &str, map: &mut HashMap<String, String>) {
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut words = line.split_whitespace();
        let Some(content_type) = words.next() else {
            continue;
        };
        for ext in words {
            map.insert(format!(".{ext}"), content_type.to_owned());
        }
    }
}

/// Returns the index of the extension in the same way as `posixpath.splitext`
/// of Python, where leading dots of the file name are not an extension.
fn extension_index(path: &str) -> Option<usize> {
    let name_start = path.rfind('/').map_or(0, |index| index + 1);
    let name = &path[name_start..];
    let index = name.rfind('.')?;
    name[..index]
        .bytes()
        .any(|b| b != b'.')
        .then_some(name_start + index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess() {
        assert_eq!(guess_content_type("a.txt"), Some("text/plain"));
        assert_eq!(guess_content_type("dir/a.TXT"), Some("text/plain"));
        assert_eq!(guess_content_type("a.json"), Some("application/json"));
        assert_eq!(guess_content_type("a.txt.gz"), Some("text/plain"));
        assert_eq!(guess_content_type("a.tgz"), Some("application/x-tar"));
        assert_eq!(guess_content_type("a.tar.gz"), Some("application/x-tar"));
        assert_eq!(guess_content_type("a.nonexistent"), None);
        assert_eq!(guess_content_type(".bashrc"), None);
        assert_eq!(guess_content_type("dir.d/x"), None);
        assert_eq!(guess_content_type("x"), None);
    }

    #[test]
    fn mime_types_file() {
        let mut map = HashMap::new();
        read_mime_types(
            r"# comment
text/plain txt log # trailing

image/png
",
            &mut map,
        );
        assert_eq!(map.get(".log").map(String::as_str), Some("text/plain"));
        assert_eq!(map.len(), 2);
    }
}
