pub use quadrant_core::models::Article;

host_commands! {
    get_news() -> Vec<Article>;
}
