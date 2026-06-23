use std::{env, u16};
use std::process::ExitCode;
use std::io::{self, BufRead, BufReader, Error, ErrorKind, BufWriter, Write};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::{Instant};

#[derive(Clone, Copy)]
#[derive(Debug)]
struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    fn ge(&self, other: &Self) -> bool {
        if self.year > other.year { return true; }
        if self.year == other.year && self.month > other.month { return true; }
        if self.year == other.year && self.month == other.month && self.day >= other.day { return true; }
        false
    }

    fn le(&self, other: &Self) -> bool {
        if self.year < other.year { return true; }
        if self.year == other.year && self.month < other.month { return true; }
        if self.year == other.year && self.month == other.month && self.day <= other.day { return true; }
        false
    }
}

struct Doc {
    path: String,
    revdate: Option<Date>,
    content: String,
    title: String,
    id: String,
    has_imagesdir: bool,
}

fn usage() {
    eprintln!(
"Usage: calendar-fast <src-paths> [options]
  -h, --help                  Print the help message.
  -v, --version               Print the version number and the build date.
  -o             PATH         Output file.
  --header       PATH         Header file.
  --footer       PATH         Footer file.
  --start-date   YYYY-MM-DD   Start date (inclusive).
  --end-date     YYYY-MM-DD   End date (inclusive).
  --imglink                   Replace images with links (will not work correctly on variable expansions).
  --order-by     revdate|title|id
");
}

fn version() {
   eprintln!("calendar-fast 0.1.0, built on 2026-06-23.");
}

fn error(text: String) -> Error {
    Error::new(ErrorKind::Other, text)
}

fn error_with_file(path: &Path, err: Error) -> Error {
    Error::new(ErrorKind::Other, format!("{}: {}", path.display(), err))
}

fn error_with_file_and_line(path: &Path, line: usize, err: Error) -> Error {
    Error::new(ErrorKind::Other, format!("{}:{}: {}", path.display(), line + 1, err))
}

fn try_parse_date(date: &str) -> io::Result<Date> {
        let len = 4 + 1 + 2 + 1 + 2;
        let mut ok = date.len() == len;

        let mut year = 0u16;
        let mut month = 0u8;
        let mut day = 0u8;

        if ok {
            let date = date.as_bytes();
            ok = date[4] == b'-' && date[7] == b'-';
        }

        if ok {
            year = date[0..=3].parse().unwrap_or_else(|_| { ok = false; 0 });
            month = date[5..=6].parse().unwrap_or_else(|_| { ok = false; 0 });
            day = date[8..=9].parse().unwrap_or_else(|_| { ok = false; 0 });

            ok = year > 0 && month >= 1 && month <= 12 && day >= 1 && day <= 31;
        }

        if !ok {
            return Err(error(format!("Could not parse date '{}'", date)));
        }

        Ok(Date {year, month, day})
}

fn try_parse_date_with_prefix(line: &str, prefix: &'static str) -> io::Result<Option<Date>> {
    if let Some(date) = line.strip_prefix(prefix) {
        match try_parse_date(date) {
            Ok(d) => Ok(Some(d)),
            Err(e) => Err(e),
        }
    } else {
        Ok(None)
    }
}

static BOM: &'static str = unsafe { std::str::from_utf8_unchecked(&[0xEF, 0xBB, 0xBF]) };

fn parse_doc(path: &Path, replace_images_with_links: bool) -> io::Result<Option<Doc>> {
    let file = File::open(path);
    if let Err(err) = file {
        return Err(error_with_file(path, err));
    }
    let file = file?;
    let lines = BufReader::new(file).lines();

    let mut cmt_block = false;
    let mut cmt_section = false;
    let mut cmt_section_block = false;

    let mut doc = Doc {
        path: path.to_string_lossy().to_string(),
        revdate: None,
        content: String::new(),
        has_imagesdir: false,
        title: String::from(""),
        id: String::from(""),
    };

    let mut doc_imagesdir: Option<String> = None;

    for (ln, line) in lines.enumerate() {
        if let Err(err) = line {
            return Err(error_with_file_and_line(path, ln, err));
        }
        let line = line?;

        let mut line_original = &line[..];
        if let Some(nb) = line_original.strip_prefix(BOM) {
            line_original = nb;
        }

        let line = line_original.trim();

        if line == "////" {
            cmt_block = !cmt_block;
        } else if line == "[comment]" {
            cmt_section = true;
        } else if cmt_section {
            if line == "--" {
                if !cmt_section_block {
                    cmt_section_block = true;
                } else {
                    cmt_section_block = false;
                    cmt_section = false;
                }
            } else if line == "" {
                if !cmt_section_block {
                    cmt_section = false
                }
            }
        }

        let mut imagesdir: Option<String> = None;

        let comment = cmt_block || cmt_section;
        if !comment {
            if line.starts_with("include::") { return Ok(None); }

            if let None = doc.revdate {
                let revdate = try_parse_date_with_prefix(line, ":revdate: ");
                if let Err(err) = revdate {
                    return Err(error_with_file_and_line(path, ln, err));
                }
                if let Some(date) = revdate? {
                    doc.revdate = Some(date);
                }
            }

            let id = line.strip_prefix(":imagesdir: ");
            if let Some(id) = id {
                imagesdir = Some(id.to_string());
            }
        }

        let mut pushed = false;
        if !comment {
            const IMAGE_PREFIX: &str = "image::";

            if replace_images_with_links && !line.starts_with("//") && line.contains(IMAGE_PREFIX) {
                let mut line_replaced: Vec<u8> = Vec::new();

                let prefix = IMAGE_PREFIX.as_bytes();
                let buf = line.as_bytes();
                let mut i = 0;
                while i < buf.len() {
                    if buf[i..].starts_with(prefix) {
                        i += prefix.len();

                        for c in "link:".bytes() {
                            line_replaced.push(c);
                        }

                        if let Some(ref dir) = doc_imagesdir {
                            for c in dir.bytes() {
                                line_replaced.push(c);
                            }

                            let db = dir.as_bytes();
                            if db[db.len() - 1] != b'/' {
                                line_replaced.push(b'/');
                            }
                        }

                        continue;
                    }

                    line_replaced.push(buf[i]);
                    i += 1;
                }

                if let Ok(line_replaced) = std::str::from_utf8(&line_replaced) {
                    doc.content.push_str(line_replaced);
                    pushed = true;
                }
            }
        }

        if !comment {
            if doc.title == "" && line.starts_with("= ") {
                doc.title = String::from(&line[2..]);
            }

            // We only treat these things before the title as ID
            if doc.title == "" && doc.id == "" {
                if line.starts_with("[#") && line.ends_with("]") {
                    doc.id = String::from(&line[2..line.len() - 1]);
                }

                if line.starts_with("[[") &&  line.ends_with("]]") {
                    doc.id = String::from(&line[2..line.len() - 2]);
                }
            }
        }

        if !pushed { doc.content.push_str(&line_original); }
        doc.content.push_str("\n");

        if let Some(dir) = imagesdir {
            doc_imagesdir = Some(dir.clone());

            doc.has_imagesdir = true;

            // If it's a variable expansion, for example
            //   {bucket}/{album}
            // we don't override the imagesdir, because
            // it may be a URL.
            // The most reliable way of doing this would be to actually keep track of the
            // variables in the document and expand them correctly, but that's some work.
            let maybe_a_variable_expansion = dir
                .chars()
                .any(|c| c == '{' || c == '}');

            let p = Path::new(&dir);
            // If we can safely assume this is a local path, we override the imagesdir
            // with the actual path so that you can get to the image.
            // TODO: This is not a very good way to determine if the path is a URL.
            // HACK: unwrap
            if !maybe_a_variable_expansion && !p.has_root() &&
               !p.starts_with("http://") && !p.starts_with("https://")
            {
                doc.content.push_str(":imagesdir: ");
                doc.content.push_str(&str::replace(path.parent().unwrap().join(p).to_str().unwrap(), "\\", "/"));
                doc.content.push_str("\n");
            }
        }
    }

    Ok(Some(doc))
}

fn generate<'a>(path: &str, header: &str, footer: &str, docs: impl Iterator<Item = &'a Doc>) -> io::Result<usize> {
    let file = File::create(path)?;
    let mut buf = BufWriter::new(file);

    let mut count_generated = 0;

    buf.write(header.as_bytes())?;
    buf.write("\n\n:leveloffset: +1\n\n".as_bytes())?;

    for doc in docs {
        if !doc.has_imagesdir {
            let p = Path::new(&doc.path);
            // TODO: unwrap
            let parent = p.parent().unwrap().to_str().unwrap();
            let mut parent = str::replace(parent, "\\", "/");

            if let Some(s) = parent.strip_prefix("//?/") {
                parent = s.to_string();
            }

            buf.write(format!(":imagesdir: {}\n", parent).as_bytes())?;
        }

        buf.write(doc.content.as_bytes())?;
        buf.write("\n\n".as_bytes())?;

        count_generated += 1;
    }

    buf.write("\n\n:leveloffset: -1\n\n".as_bytes())?;
    buf.write(footer.as_bytes())?;

    Ok(count_generated)
}

fn get_adoc_files(path: &Path, files: &mut HashSet<PathBuf>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            get_adoc_files(&path, files)?;
        }
    } else if path.is_file() {
        let ext = path.extension();
        if ext.is_none() {
            return Ok(());
        } else if let Some(ext) = ext {
            if ext.to_str() != Some("adoc") {
                return Ok(());
            }
        }
        files.insert(fs::canonicalize(path).unwrap());
    }

    Ok(())
}

enum OrderBy {
    Revdate,
    Title,
    ID,
}

fn main() -> ExitCode {
    let perf_total = Instant::now();

    let mut args = env::args();
    args.next().unwrap();

    let mut src_dirs: Vec<String> = Vec::new();

    let mut out_path = String::from("calendar.adoc");
    let mut header_path: Option<String> = None;
    let mut footer_path: Option<String> = None;

    let mut start_date = Date { year: 0, month: 0, day: 0 };
    let mut end_date = Date { year: u16::MAX, month: u8::MAX, day: u8::MAX };
    let mut date_bounds_specified = false;

    let mut replace_images_with_links = false;

    let mut order_by = OrderBy::Revdate;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            "-v" | "--version" => {
                version();
                return ExitCode::SUCCESS;
            }
            "--header" => {
                match args.next() {
                    Some(path) => header_path = Some(path),
                    None => {
                        eprintln!("Error: You typed --header, but didn't specify what the file is afterwards.");
                        return ExitCode::from(1);
                    },
                }
            }
            "--footer" => {
                match args.next() {
                    Some(path) => footer_path = Some(path),
                    None => {
                        eprintln!("Error: You typed --footer, but didn't specify what the file is afterwards.");
                        return ExitCode::from(1);
                    },
                }
            }
            "-o" => {
                match args.next() {
                    Some(path) => out_path = path,
                    None => {
                        eprintln!("Error: You typed -o, but didn't specify what the file is afterwards.");
                        return ExitCode::from(1);
                    },
                }
            }
            "--start-date" => {
                start_date = match try_parse_date(&args.next().unwrap()) {
                    Ok(d) => {
                        date_bounds_specified = true;
                        d
                    },
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                }
            }
            "--end-date" => {
                end_date = match try_parse_date(&args.next().unwrap()) {
                    Ok(d) => {
                        date_bounds_specified = true;
                        d
                    },
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                }
            }
            "--imglink" => {
                replace_images_with_links = true;
            }
            "--order-by" => {
                order_by = match args.next() {
                    Some(what) => {
                        match what.as_str() {
                            "revdate" => OrderBy::Revdate,
                            "title" => OrderBy::Title,
                            "id" => OrderBy::ID,
                            &_ => {
                                eprintln!("Error: --order-by is either 'revdate', 'title', or 'id'.");
                                return ExitCode::from(1);
                            }
                        }
                    }
                    None => {
                        eprintln!("Error: You typed --order-by, but didn't specify what to order by.");
                        return ExitCode::from(1);
                    }
                }
            }
            _ => {
                src_dirs.push(arg);
            }
        }
   }

    if src_dirs.len() == 0 {
        usage();
        eprintln!("Error: No source directories provided.");
        return ExitCode::from(1);
    }

    // TODO: unwrap

    let header = if let Some(path) = header_path {
        fs::read_to_string(path).unwrap()
    } else {
        String::from("= Calendar\n\n")
    };

    let footer = if let Some(path) = footer_path {
        fs::read_to_string(path).unwrap()
    } else {
        String::from("")
    };

    let perf_traverse = Instant::now();

    let mut files: HashSet<PathBuf> = HashSet::new();

    for dir in src_dirs {
        let path = Path::new(&dir);

        if !path.exists() {
            eprintln!("Error: Source directory '{}' does not exist.", path.display());
            return ExitCode::from(1);
        }

        if !path.is_dir() {
            eprintln!("Error: Source path '{}' is not a directory.", path.display());
            return ExitCode::from(1);
        }

        match get_adoc_files(path, &mut files) {
            Ok(_) => {},
            Err(err) => {
                eprintln!("Error: {err}");
                return ExitCode::from(1);
            }
        };
    }

    let perf_traverse = perf_traverse.elapsed();

    println!("AsciiDoc files found: {}.", files.len());

    let perf_parse = Instant::now();

    let mut docs: Vec<Doc> = Vec::new();
    for path in files {
        let doc = parse_doc(&path, replace_images_with_links).unwrap();
        if let Some(doc) = doc {
            docs.push(doc);
        } else {
            // It had include::[].
        }
    }

    let perf_parse = perf_parse.elapsed();

    let perf_output = Instant::now();

    match order_by {
        OrderBy::Revdate => {
            docs.sort_by(|a, b| {
                // Sort by revdates in descending order (newest on the top).

                let l = a.revdate;
                let r = b.revdate;

                if l.is_none() && r.is_none() {
                    return Ordering::Equal;
                } else if l.is_none() {
                    return Ordering::Greater;
                } else if r.is_none() {
                    return Ordering::Less;
                }

                let l = l.unwrap();
                let r = r.unwrap();

                let y = r.year.cmp(&l.year);
                let m = r.month.cmp(&l.month);
                let d = r.day.cmp(&l.day);

                if y != Ordering::Equal { return y; }
                if m != Ordering::Equal { return m; }
                if d != Ordering::Equal { return d; }

                Ordering::Equal
            });
        }

        OrderBy::Title => {
            docs.sort_by(|a, b| {
                let l = &a.title;
                let r = &b.title;

                if l == "" && r == "" {
                    return Ordering::Equal;
                } else if l == "" {
                    return Ordering::Greater;
                } else if r == "" {
                    return Ordering::Less;
                }

                l.cmp(&r)
            });
        }

        OrderBy::ID => {
            docs.sort_by(|a, b| {
                let l = &a.id;
                let r = &b.id;

                if l == "" && r == "" {
                    return Ordering::Equal;
                } else if l == "" {
                    return Ordering::Greater;
                } else if r == "" {
                    return Ordering::Less;
                }

                l.cmp(&r)
            });
        }
    }

    let docs_filtered = docs.iter().filter(|doc| {
        if let Some(date) = doc.revdate {
            date.ge(&start_date) && date.le(&end_date)
        } else {
            !date_bounds_specified
        }
    });

    match generate(&out_path, &header, &footer, docs_filtered) {
        Ok(count) => {
            println!("Documents   included: {count}.");
        },
        Err(err) => {
            eprintln!("Error: {err}");
            return ExitCode::from(1);
        }
    };

    let perf_output = perf_output.elapsed();

    let perf_total = perf_total.elapsed();

    println!("");
    println!("Traverse time: {:.5} s.", perf_traverse.as_secs_f32());
    println!("Parse    time: {:.5} s.", perf_parse.as_secs_f32());
    println!("Output   time: {:.5} s.", perf_output.as_secs_f32());
    println!("Other    time: {:.5} s.", (perf_total - (perf_traverse + perf_parse + perf_output)).as_secs_f32());
    println!("Total    time: {:.5} s.", perf_total.as_secs_f32());

    ExitCode::SUCCESS
}
