use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use chrono::{DateTime, Datelike, FixedOffset, Local};
use serde_json::json;
use sqlx::{SqliteConnection, SqlitePool};
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tera::{Context, Tera};
use tokio::net::TcpListener;
use tracing::info;
use uuid::Uuid;

macro_rules! fatal {
    ($($arg:tt)*) => {{
        ::tracing::error!($($arg)*);
        ::anyhow::bail!($($arg)*);
    }};
}

#[derive(Debug, ts_rs::TS, serde::Serialize, sqlx::FromRow)]
#[ts(export)]
struct Post {
    id: Uuid,
    title: String,
    subtitle: Option<String>,
    published: DateTime<FixedOffset>,
    content: String,
}

impl Post {
    fn slug(&self) -> String {
        let short = if self.title.len() > 26 {
            &self.title[..26]
        } else {
            &self.title
        };

        slug::slugify(short)
            + &format!(
                "-{:04}-{:02}-{:02}",
                self.published.year(),
                self.published.month(),
                self.published.day()
            )
    }
}

const DOT_DIR: &str = ".blog3";

#[derive(Debug, serde::Deserialize)]
struct Config {
    page_root: String,
    bind: SocketAddr,
    database: PathBuf,
    #[serde(default)]
    basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct BasicAuthConfig {
    user: String,
    password: String,
    realm: Option<String>,
}

impl Config {
    fn route(&self, child: &str) -> String {
        self.page_root.clone() + child
    }

    fn route_dot(&self, child: &str) -> String {
        self.page_root.clone() + "/" + DOT_DIR + child
    }
}

struct App {
    config: Config,
    pool: SqlitePool,
    tera: Tera,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    run().await.unwrap();
}

const PAGE_TEMPLATE: &str = "page";

async fn run() -> Result<()> {
    let Some(config) = std::env::args().nth(1) else {
        fatal!("missing config path filename");
    };
    let config = tokio::fs::read_to_string(config).await?;
    let config: Config = match toml::from_str(&config) {
        Ok(config) => config,
        Err(err) => fatal!("{}", err),
    };
    info!("{:#?}", config);

    let mut app = App {
        pool: SqlitePool::connect(&format!("sqlite:{}", config.database.display())).await?,
        tera: Tera::default(),
        config,
    };

    app.tera.add_raw_template(
        PAGE_TEMPLATE,
        include_str!("../frontend/src/page.html.tera"),
    )?;

    let bind = app.config.bind.clone();
    let app = Arc::new(app);

    let authed_router = Router::new()
        .route(&app.config.route_dot("/publish"), post(publish_handler))
        .route(
            &app.config.route_dot("/publish/{update}"),
            post(update_handler),
        )
        .route(&app.config.route("/edit/{page}"), get(edit_handler))
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            basic_auth_layer,
        ))
        .with_state(app.clone());

    let router = Router::new()
        .route(&app.config.route_dot("/assets/{item}"), get(assets_handler))
        .route(&app.config.route("/{slug}"), get(page_handler))
        .with_state(app.clone())
        .merge(authed_router);

    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

async fn basic_auth_layer(
    State(app): State<Arc<App>>,
    basic_auth: Option<TypedHeader<Authorization<Basic>>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    match (app.config.basic_auth.as_ref(), basic_auth) {
        (Some(BasicAuthConfig { user, password, .. }), Some(TypedHeader(header))) => {
            if header.username() == user && header.password() == password {
                tracing::trace!("Successful basic auth");
                next.run(request).await
            } else {
                (StatusCode::UNAUTHORIZED, "Incorrect username/password").into_response()
            }
        }

        (Some(BasicAuthConfig { realm, .. }), None) => (
            StatusCode::UNAUTHORIZED,
            [(
                axum::http::header::WWW_AUTHENTICATE,
                &format!(
                    "Basic realm=\"{}\"",
                    realm.as_deref().unwrap_or("mycoolblog")
                ),
            )],
            "Need auth",
        )
            .into_response(),

        (None, _) => next.run(request).await,
    }
}

async fn assets_handler(Path(item): Path<String>) -> Response {
    // 1 year by default
    macro_rules! response {
        ($name:literal => $content_type:literal $file:literal) => {
            response!($name => $content_type $file "max-age=31536000, immutable")
        };

        ($name:literal => $content_type:literal $file:literal $cache:literal) => {
            if item == $name {
                return (
                    [
                        ("Content-Type", $content_type),
                        ("Cache-Control", $cache),
                    ],
                    include_bytes!($file),
                )
                    .into_response();
            }
        };
    }

    response!("page.js" => "text/javascript" "../frontend/build/page.js" "max-age=3600, must-revalidate");
    response!("page.css" => "text/css" "../frontend/src/page.css" "max-age=3600, must-revalidate");

    response!("apple-touch-icon.png" => "image/png" "../frontend/assets/apple-touch-icon.png");
    response!("favicon-96x96.png" => "image/png" "../frontend/assets/favicon-96x96.png");
    response!("favicon.ico" => "image/x-icon" "../frontend/assets/favicon.ico");
    response!("favicon.svg" => "image/svg+xml" "../frontend/assets/favicon.svg");
    response!("web-app-manifest-192x192.png" => "image/png" "../frontend/assets/web-app-manifest-192x192.png");
    response!("web-app-manifest-512x512.png" => "image/png" "../frontend/assets/web-app-manifest-512x512.png");

    #[cfg(debug_assertions)]
    {
        response!("page.js.map" => "text/javascript" "../frontend/build/page.js.map")
    }

    StatusCode::NOT_FOUND.into_response()
}

#[derive(Debug, ts_rs::TS, serde::Deserialize)]
#[ts(export)]
struct Publish {
    title: String,
    subtitle: Option<String>,
    content: String,
}

#[tracing::instrument(skip_all)]
async fn publish_handler(
    State(app): State<Arc<App>>,
    Json(to_publish): Json<Publish>,
) -> Response {
    let post = Post {
        id: Uuid::new_v4(),
        title: to_publish.title,
        subtitle: to_publish.subtitle,
        published: Local::now().fixed_offset(),
        content: to_publish.content,
    };

    tracing::trace!(new_post = ?post);

    let mut tx = match app.pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(new_post_transaction = %err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    if let Err(err) = app.insert_post(&mut *tx, &post).await {
        tracing::error!(insert_post = %err);
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    // insert a slug
    let slug = post.slug();
    let posts_with_slug = match app.count_ids_with_similar_slugs(&mut *tx, &slug).await {
        Ok(slug) => slug,
        Err(err) => {
            tracing::error!(new_post_slug = %err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    let slug = if posts_with_slug > 0 {
        format!("{slug}-{posts_with_slug}")
    } else {
        slug
    };

    if let Err(err) = app.insert_slug(&mut *tx, &slug, post.id).await {
        tracing::error!(insert_slug = %err);
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    if let Err(err) = tx.commit().await {
        tracing::error!(new_post_transaction_commit = %err);
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    Json(json!({ "id": post.id, "slug": slug })).into_response()
}

#[tracing::instrument(skip_all)]
async fn update_handler(
    State(app): State<Arc<App>>,
    Path(update): Path<Uuid>,
    Json(to_publish): Json<Publish>,
) -> Response {
    let mut tx = match app.pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(update_post_transaction = %err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    match app.find_post(&mut *tx, update).await {
        Ok(Some(existing)) => {
            tracing::trace!(update_existing = %update);

            // have an existing post, copy it into old. TODO make this not a json string
            if let Err(err) = app.insert_old(&mut *tx, &existing).await {
                tracing::error!(insert_old = %err);
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            };

            let new_post = Post {
                id: existing.id,
                title: to_publish.title,
                subtitle: to_publish.subtitle,
                published: Local::now().fixed_offset(),
                content: to_publish.content,
            };

            // update the existing post
            if let Err(err) = app.update_post(&mut *tx, &new_post).await {
                tracing::error!(update_existing = %err);
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }

            let slug = new_post.slug();
            let ids_with_slug = match app.find_ids_with_similar_slugs(&mut *tx, &slug).await {
                Ok(posts) => posts,
                Err(err) => {
                    tracing::error!(new_post_slug = %err);
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
            };

            let renaming_to_new_slug = !ids_with_slug.contains_key(&new_post.id);

            tracing::trace!(try_slug = %slug, ids_with_slug = ?ids_with_slug, ?renaming_to_new_slug);

            let slug = if ids_with_slug.len() > 0 && renaming_to_new_slug {
                format!("{slug}-{}", ids_with_slug.len())
            } else if !renaming_to_new_slug {
                // SAFETY: should already exist if we're renaming to an existing slug
                ids_with_slug[&new_post.id].clone()
            } else {
                slug
            };

            tracing::trace!(updated_slug = %slug);

            if renaming_to_new_slug
                && let Err(err) = app.insert_slug(&mut *tx, &slug, new_post.id).await
            {
                tracing::error!(update_slug = %err);
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            };

            if let Err(err) = app.update_old_slugs(&mut *tx, new_post.id, &slug).await {
                tracing::error!(update_old_slug = %err);
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }

            if let Err(err) = tx.commit().await {
                tracing::error!(update_post_transaction_commit = %err);
                return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
            }

            Json(json!({ "id": new_post.id, "slug": slug })).into_response()
        }

        // passed a uuid in the path but the post with that uuid didn't exist
        Ok(None) => {
            tracing::trace!(not_found = %update);
            (StatusCode::NOT_FOUND, "post not found").into_response()
        }

        Err(err) => {
            tracing::error!(select_existing = %err);
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }
}

async fn edit_handler(State(app): State<Arc<App>>, Path(page): Path<String>) -> Response {
    "edit".into_response()
}

async fn page_handler(State(app): State<Arc<App>>, Path(slug): Path<String>) -> Response {
    let mut tx = match app.pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!(page_handler_transaction = %err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    match app.get_newest_slug(&mut *tx, &slug).await {
        Ok(Some((id, newslug))) => {
            if newslug != slug {
                tracing::debug!(redirected = %slug, to = %newslug);
                return (
                    StatusCode::MOVED_PERMANENTLY,
                    [("Location", app.config.route(&format!("/{newslug}")))],
                )
                    .into_response();
            }

            match app.find_post(&mut *tx, id).await {
                Ok(Some(post)) => {
                    let mut context = Context::new();

                    context.insert("post", &post);
                    context.insert("page_root", &app.config.page_root);

                    match app.tera.render(PAGE_TEMPLATE, &context) {
                        Ok(rendered) => Html(rendered).into_response(),
                        Err(err) => {
                            tracing::error!(render_page = ?err, post = %id, %slug);
                            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
                        }
                    }
                }

                Ok(None) => {
                    tracing::error!(find_post_returned_nothing_wat = %id, %newslug, oldslug = %slug);
                    (StatusCode::INTERNAL_SERVER_ERROR, "page not in database?").into_response()
                }

                Err(err) => {
                    tracing::error!(page_handler_find_post = %err);
                    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
                }
            }
        }

        Ok(None) => (StatusCode::NOT_FOUND, "todo: nice 404 page").into_response(),

        Err(err) => {
            tracing::error!(get_newest_slug_page_handler = %err);
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }
}

impl App {
    async fn insert_post(&self, conn: &mut SqliteConnection, post: &Post) -> Result<()> {
        sqlx::query!(
            "insert into post (id, title, subtitle, published, content) values ($1, $2, $3, $4, $5)",
            post.id,
            post.title,
            post.subtitle,
            post.published,
            post.content,
        )
        .execute(conn)
        .await?;

        Ok(())
    }

    async fn insert_slug(&self, conn: &mut SqliteConnection, slug: &str, id: Uuid) -> Result<()> {
        sqlx::query!("insert into slug (slug, id) values ($1, $2)", slug, id)
            .execute(conn)
            .await?;
        Ok(())
    }

    async fn get_newest_slug(
        &self,
        conn: &mut SqliteConnection,
        slug: &str,
    ) -> Result<Option<(Uuid, String)>> {
        let row = sqlx::query!("select id, newslug from slug where slug = $1", slug)
            .fetch_optional(conn)
            .await?;

        Ok(row.map(|row| {
            (
                Uuid::from_slice(&row.id).expect("valid uuids in database"),
                row.newslug.unwrap_or_else(|| String::from(slug)),
            )
        }))
    }

    async fn find_post(&self, conn: &mut SqliteConnection, id: Uuid) -> Result<Option<Post>> {
        let post = sqlx::query_as::<_, Post>("select * from post where id = $1 limit 1")
            .bind(&id)
            .fetch_optional(conn)
            .await?;
        Ok(post)
    }

    async fn insert_old(&self, conn: &mut SqliteConnection, post: &Post) -> Result<()> {
        let old = serde_json::to_string(&post).expect("post is valid json");
        sqlx::query!("insert into old (id, data) values ($1, $2)", post.id, old,)
            .execute(conn)
            .await?;
        Ok(())
    }

    async fn update_post(&self, conn: &mut SqliteConnection, post: &Post) -> Result<()> {
        sqlx::query!(
            r#"
                update post
                    set title = $1,
                        subtitle = $2,
                        published = $3,
                        content = $4
                    where id = $5
            "#,
            post.title,
            post.subtitle,
            post.published,
            post.content,
            post.id
        )
        .execute(conn)
        .await?;
        Ok(())
    }

    async fn count_ids_with_similar_slugs(
        &self,
        conn: &mut SqliteConnection,
        slug: &str,
    ) -> Result<usize> {
        Ok(self.find_ids_with_similar_slugs(conn, slug).await?.len())
    }

    async fn find_ids_with_similar_slugs(
        &self,
        conn: &mut SqliteConnection,
        slug: &str,
    ) -> Result<HashMap<Uuid, String>> {
        let slug_like = format!("{slug}%");

        let row = sqlx::query!("select id, slug from slug where slug like $1", slug_like)
            .fetch_all(conn)
            .await?;

        Ok(row
            .into_iter()
            .map(|row| {
                (
                    Uuid::from_slice(&row.id).expect("valid uuids in database"),
                    row.slug,
                )
            })
            .collect())
    }

    async fn update_old_slugs(
        &self,
        conn: &mut SqliteConnection,
        id: Uuid,
        new_slug: &str,
    ) -> Result<()> {
        sqlx::query!("update slug set newslug = $1 where id = $2", new_slug, id)
            .execute(conn)
            .await?;

        Ok(())
    }
}
