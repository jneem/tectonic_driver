extern crate clap;
#[macro_use] extern crate tectonic;
extern crate tectonic_driver;

use clap::{Arg, ArgMatches, App};
use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process;

use tectonic::config::PersistentConfig;
use tectonic::errors::{ErrorKind, Result, ResultExt};
use tectonic::io::itarbundle::{HttpITarIoFactory, ITarBundle};
use tectonic::io::zipbundle::ZipBundle;
use tectonic::status::{ChatterLevel, StatusBackend};
use tectonic::status::termcolor::TermcolorStatusBackend;

use tectonic_driver::{CliIoBuilder, OutputFormat, PassSetting, ProcessingSessionBuilder};

fn inner(args: ArgMatches, config: PersistentConfig, status: &mut TermcolorStatusBackend) -> Result<i32> {
    let tex_path = args.value_of_os("INPUT").unwrap();

    let output_format = match args.value_of("outfmt").unwrap() {
        "aux" => OutputFormat::Aux,
        "html" => OutputFormat::Html,
        "xdv" => OutputFormat::Xdv,
        "pdf" => OutputFormat::Pdf,
        "format" => OutputFormat::Format,
        _ => unreachable!()
    };

    let pass = match args.value_of("pass").unwrap() {
        "default" => PassSetting::Default,
        "bibtex_first" => PassSetting::BibtexFirst,
        "tex" => PassSetting::Tex,
        _ => unreachable!()
    };

    // Input and path setup

    let mut io_builder = CliIoBuilder::default();
    let mut output_path;
    let tex_input_stem;

    if tex_path == "-" {
        io_builder.primary_input_stdin();

        output_path = PathBuf::new();
        tex_input_stem = OsStr::new("texput.tex");
        tt_note!(status, "reading from standard input; outputs will appear under the base name \"texput\"");
    } else {
        let tex_path = Path::new(&tex_path);
        io_builder.primary_input_path(&tex_path);

        tex_input_stem = match tex_path.file_name() {
            Some(fname) => fname,
            None => { return Err(ErrorKind::Msg(format!("can't figure out a basename for input path \"{}\"",
                                                        tex_path.to_string_lossy())).into()); },
        };

        if let Some(par) = tex_path.parent() {
            output_path = par.to_owned();
            io_builder.filesystem_root(par);
        } else {
            return Err(ErrorKind::Msg(format!("can't figure out a parent directory for input path \"{}\"",
                                              tex_path.to_string_lossy())).into());
        }
    }

    if let Some(dir) = args.value_of_os("outdir") {
        output_path = PathBuf::from(dir);
        if !output_path.is_dir() {
            return Err(ErrorKind::Msg(format!("output directory \"{}\" does not exist", output_path.display())).into());
        }
    }

    // Set up the rest of I/O.

    io_builder.use_genuine_stdout(args.is_present("print_stdout"));

    if let Some(items) = args.values_of_os("hide") {
        for v in items {
            io_builder.hide_path(v);
        }
    }

    if let Some(p) = args.value_of("bundle") {
        let zb = ctry!(ZipBundle::<File>::open(Path::new(&p)); "error opening bundle");
        io_builder.bundle(zb);
    } else if let Some(u) = args.value_of("web_bundle") {
        let tb = ITarBundle::<HttpITarIoFactory>::new(&u);
        io_builder.bundle(tb);
    } else {
        io_builder.boxed_bundle(config.default_io_provider(status)?);
    }

    let mut builder = ProcessingSessionBuilder::new();
    builder.format_path(args.value_of("format").unwrap())
        .output_path(&output_path)
        .tex_input_stem(&tex_input_stem.to_string_lossy())
        .output_format(output_format)
        .pass(pass)
        .keep_intermediates(args.is_present("keep_intermediates"))
        .keep_logs(args.is_present("keep_logs"))
        .synctex(args.is_present("syntex"));

    if let Some(reruns) = args.value_of("reruns") {
        builder.reruns(usize::from_str_radix(reruns, 10)?);
    }
    if let Some(m) = args.value_of_os("makefile_rules") {
        builder.makefile_output_path(m);
    }

    let mut sess = builder.create(&config, io_builder)?;
    sess.run(status)
}


fn main() {
    let matches = App::new("Tectonic")
        .version("0.1.8-dev")
        .about("Process a (La)TeX document.")
        .arg(Arg::with_name("format")
             .long("format")
             .value_name("PATH")
             .help("The name of the \"format\" file used to initialize the TeX engine.")
             .default_value("latex"))
        .arg(Arg::with_name("bundle")
             .long("bundle")
             .short("b")
             .value_name("PATH")
             .help("Use this Zip-format bundle file to find resource files instead of the default.")
             .takes_value(true))
        .arg(Arg::with_name("web_bundle")
             .long("web-bundle")
             .short("w")
             .value_name("URL")
             .help("Use this URL find resource files instead of the default.")
             .takes_value(true))
        .arg(Arg::with_name("outfmt")
             .long("outfmt")
             .value_name("FORMAT")
             .help("The kind of output to generate.")
             .possible_values(&["pdf", "html", "xdv", "aux", "format"])
             .default_value("pdf"))
        .arg(Arg::with_name("makefile_rules")
             .long("makefile-rules")
             .value_name("PATH")
             .help("Write Makefile-format rules expressing the dependencies of this run to <PATH>."))
        .arg(Arg::with_name("pass")
             .long("pass")
             .value_name("PASS")
             .help("Which engines to run.")
             .possible_values(&["default", "tex", "bibtex_first"])
             .default_value("default"))
        .arg(Arg::with_name("reruns")
             .long("reruns")
             .short("r")
             .value_name("COUNT")
             .help("Rerun the TeX engine exactly this many times after the first."))
        .arg(Arg::with_name("keep_intermediates")
             .short("k")
             .long("keep-intermediates")
             .help("Keep the intermediate files generated during processing."))
        .arg(Arg::with_name("keep_logs")
             .long("keep-logs")
             .help("Keep the log files generated during processing."))
        .arg(Arg::with_name("synctex")
             .long("synctex")
             .help("Generate SyncTeX data."))
        .arg(Arg::with_name("hide")
             .long("hide")
             .value_name("PATH")
             .multiple(true)
             .number_of_values(1)
             .help("Tell the engine that no file at <PATH> exists, if it tries to read it."))
        .arg(Arg::with_name("print_stdout")
             .long("print")
             .short("p")
             .help("Print the engine's chatter during processing."))
        .arg(Arg::with_name("chatter_level")
             .long("chatter")
             .short("c")
             .value_name("LEVEL")
             .help("How much chatter to print when running.")
             .possible_values(&["default", "minimal"])
             .default_value("default"))
        .arg(Arg::with_name("outdir")
             .long("outdir")
             .short("o")
             .value_name("OUTDIR")
             .help("The directory in which to place output files. [default: the directory containing INPUT]"))
        .arg(Arg::with_name("INPUT")
             .help("The file to process.")
             .required(true)
             .index(1))
        .get_matches ();

    let chatter = match matches.value_of("chatter_level").unwrap() {
        "default" => ChatterLevel::Normal,
        "minimal" => ChatterLevel::Minimal,
        _ => unreachable!()
    };

    // I want the CLI program to take as little configuration as possible, but
    // we do need to at least provide a mechanism for storing the default
    // bundle.

    let config = match PersistentConfig::open(false) {
        Ok(c) => c,
        Err(ref e) => {
            // Uhoh, we couldn't get the configuration. Our main
            // error-printing code requires a 'status' object, which we don't
            // have yet. If we can't even load the config we might really be
            // in trouble, so it seems safest to keep things simple anyway and
            // just use bare stderr without colorization.
            e.dump_uncolorized();
            process::exit(1);
        }
    };

    // Set up colorized output. This comes after the config because you could
    // imagine wanting to be able to configure the colorization (which is
    // something I'd be relatively OK with since it'd only affect the progam
    // UI, not the processing results).

    let mut status = TermcolorStatusBackend::new(chatter);

    // For now ...

    tt_note!(status, "this is a BETA release; ask questions and report bugs at https://tectonic.newton.cx/");

    // Now that we've got colorized output, we're to pass off to the inner
    // function ... all so that we can print out the word "error:" in red.
    // This code parallels various bits of the `error_chain` crate.

    process::exit(match inner(matches, config, &mut status) {
        Ok(ret) => ret,

        Err(ref e) => {
            status.bare_error(e);
            1
        }
    })
}
