use std::borrow::Cow;
use std::fs::read_to_string;
use std::path::PathBuf;
use minijinja::{self, Environment, Source};
use actix_web::{get, web, App, HttpServer, Responder, HttpResponse, http, HttpRequest};
use serde::{Deserialize, Serialize};
use serde_json;
use serde_json::json;
use clap::{Parser};
use anyhow;
use log::{debug, error, info};
use env_logger::{self, Env};


const TEMPLATE_NAME: &'static str = "pbar_template";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Sets a custom config file
    #[arg(short='f', long)]
    template_file: Option<PathBuf>,

    #[clap(short, long, value_parser, default_value="127.0.0.1")]
    /// Bind address.
    ip: String,

    #[clap(short, long, value_parser=clap::value_parser!(u16).range(1..), default_value_t=5005)]
    /// The port to listen on.
    port: u16,

    #[clap(short, long, value_parser=clap::value_parser!(u16).range(1..), default_value_t=1)]
    /// The port to listen on.
    workers: u16,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // env_logger::init();
    env_logger::Builder::from_env(Env::default()
        .default_filter_or(concat!(module_path!(), "=info")))
        .init();

    let cli = Cli::parse();

    let mut env = Environment::new();
    match &cli.template_file {
        Some(file) => {
            let mut source = Source::new();
            source.add_template(TEMPLATE_NAME, read_to_string(file)?)?;
            env.set_source(source);
        },
        None => {
            let template = include_str!("../resources/default.svg");
            env.add_template(TEMPLATE_NAME, template)?;
        },
    };
    env.add_filter("int", |x: f32| x as i32);

    info!("{} {} at {}:{}.",
        cli.workers, if cli.workers > 1 { "workers serve" } else { "worker serves" },
        cli.ip, cli.port);

    let data = web::Data::new(env);
    HttpServer::new(move ||
        App::new()
            .app_data(data.clone())
            .service(serve_progress_svg_image))
        .workers(cli.workers as usize)
        .bind((cli.ip, cli.port))?
        .run()
        .await?;
    Ok(())
}


#[derive(Deserialize, Serialize)]
struct QueryArgs {
    title: Option<String>,
    title_width: Option<i32>,
    title_color: Option<Cow<'static, str>>,
    scale: Option<f32>,
    progress: f32,
    progress_width: Option<i32>,
    progress_color: Option<Cow<'static, str>>,
    suffix: Option<Cow<'static, str>>,
}

#[get("/")]
async fn serve_progress_svg_image(
    args: web::Query<QueryArgs>,
    env: web::Data<Environment<'_>>,
    req: HttpRequest
) -> impl Responder {
    let log_header = format!(
        "request from {} with query {}",
        req.peer_addr().map_or(Cow::from("<UNKNOWN>"),
                               |x| x.ip().to_string().into()),
        req.uri());

    let template = match env.get_template(TEMPLATE_NAME) {
        Ok(x) => x,
        Err(e) => {
            error!("{} -> Failed to find template. It probably a bug. \
            Please report it to the Developer. {}", log_header, e);
            return HttpResponse::build(http::StatusCode::INTERNAL_SERVER_ERROR)
                .content_type("text/plain; charset=utf-8")
                .body(format!("Failed to find template. It probably a bug. \
                Please report it to the Developer. {e}"))
        }
    };

    // println!("{template:?}");
    // let ctx = context!{
    //     progress => 50.0,
    //     title_width => 0,
    //     progress_color => "#f0ad4e",
    //     progress_width => 90,
    //     scale => 100.0,
    //     suffix => "%",
    //     title_color => "#428bca"
    // };
    // println!("{ctx}");
    //
    // let src = template.render(ctx).unwrap();
    // println!("{src}");

    let ctx = extract_template_fields(args.into_inner());
    debug!("{} - Parsed query arguments: {}", log_header, ctx);

    return if let Ok(x) = template.render(&ctx) {
        info!("{} - OK", log_header);
        HttpResponse::build(http::StatusCode::OK)
            .content_type("image/svg+xml; charset=utf-8")
            .body(x)
    } else {
        error!("{} - Failed. Probably bad query parameters", log_header);
        HttpResponse::build(http::StatusCode::BAD_REQUEST)
            .content_type("text/plain; charset=utf-8")
            .body(format!("Failed to construct progress bar with parameters: {ctx}"))
    }
}


fn get_progress_color(progress: f32, scale: f32) -> &'static str {
    let ratio = progress / scale;

    return if ratio < 0.3 {
        "#d9534f"
    } else if ratio < 0.7 {
        "#f0ad4e"
    } else {
        "#5cb85c"
    }
}

fn extract_template_fields(query: QueryArgs) -> minijinja::value::Value {
    let mut args = json!({});
    let mut progress_width = 90;
    let mut title_width = 0;

    if let Some(title) = query.title {
        progress_width = 60;
        title_width = 10 + 6 * title.len() as i32;
        args["title"] = title.into();
    }

    if let Some(width) = query.title_width {
        args["title_width"] = width.into();
    }

    let scale = query.scale.unwrap_or(100.0);
    args["title_color"] = query.title_color.unwrap_or_else(|| "#428bca".into()).into();
    args["title_width"] = query.title_width.unwrap_or(title_width).into();
    args["scale"] = scale.into();
    args["progress"] = query.progress.into();
    args["progress_width"] = query.progress_width.unwrap_or(progress_width).into();
    args["progress_color"] = query.progress_color.unwrap_or_else(||
        get_progress_color(query.progress, scale).into()).into();
    args["suffix"] = query.suffix.unwrap_or_else(|| "%".into()).into();

    minijinja::value::Value::from_serializable(&args)
}
