//! graphql-js v17.0.2 `src/execution/__tests__/schema-test.ts`, case by case
//! in upstream order.

use crate::common::*;
use greem::ExecuteOptions;
use greem_compliance::graphql_js::schema::{Article, Author, World};
use greem_compliance::harness::Area;
use serde_json::{Value, json};

/// Upstream's `article(id)`. Its `keywords` hold what upstream's mixed list
/// coerces to as `String`s (`1` and `true` become their text); its author's
/// id is the text of upstream's `123`.
fn article(id: u32) -> Article {
    Article {
        id: id.to_string(),
        is_published: true,
        author: Author {
            id: "123".into(),
            name: "John Smith".into(),
            pic_url: "cdn://123".into(),
            recent_article: "1".into(),
        },
        title: format!("My Article {id}"),
        body: "This is a post".into(),
        keywords: vec![
            Some("foo".into()),
            Some("bar".into()),
            Some("1".into()),
            Some("true".into()),
            None,
        ],
    }
}

fn blog() -> World {
    World {
        articles: (1..=10).map(article).collect(),
        ..Default::default()
    }
}

/// describe('Execute: Handles execution with a complex schema')
mod handles_execution_with_a_complex_schema {
    use super::*;

    /// it('executes using a schema')
    ///
    /// Upstream executes without validating so that `hidden` and
    /// `notDefined`, which the schema does not declare, are silently left
    /// out; greem validates at parse, so the document is a request error.
    /// The second half of the test keeps the execution claim over the
    /// same document without those two fields.
    #[test]
    fn executes_using_a_schema() {
        let (v, _) = blog().single(
            r#"
      {
        feed {
          id,
          title
        },
        article(id: "1") {
          ...articleFields,
          author {
            id,
            name,
            pic(width: 640, height: 480) {
              url,
              width,
              height
            },
            recentArticle {
              ...articleFields,
              keywords
            }
          }
        }
      }

      fragment articleFields on Article {
        id,
        isPublished,
        title,
        body,
        hidden,
        notDefined
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        // apollo-compiler's wording, learnt by running greem.
        assert_response(
            &v,
            &json!({
                "errors": [
                    {
                        "message": "type `Article` does not have a field `hidden`",
                        "locations": [{"line": 30, "column": 9}],
                    },
                    {
                        "message": "type `Article` does not have a field `notDefined`",
                        "locations": [{"line": 31, "column": 9}],
                    },
                ],
            }),
        );

        let (v, _) = blog().single(
            r#"
      {
        feed {
          id,
          title
        },
        article(id: "1") {
          ...articleFields,
          author {
            id,
            name,
            pic(width: 640, height: 480) {
              url,
              width,
              height
            },
            recentArticle {
              ...articleFields,
              keywords
            }
          }
        }
      }

      fragment articleFields on Article {
        id,
        isPublished,
        title,
        body
      }
    "#,
            Value::Null,
            ExecuteOptions::default(),
        );
        assert_response(
            &v,
            &json!({
                "data": {
                    "feed": [
                        {"id": "1", "title": "My Article 1"},
                        {"id": "2", "title": "My Article 2"},
                        {"id": "3", "title": "My Article 3"},
                        {"id": "4", "title": "My Article 4"},
                        {"id": "5", "title": "My Article 5"},
                        {"id": "6", "title": "My Article 6"},
                        {"id": "7", "title": "My Article 7"},
                        {"id": "8", "title": "My Article 8"},
                        {"id": "9", "title": "My Article 9"},
                        {"id": "10", "title": "My Article 10"},
                    ],
                    "article": {
                        "id": "1",
                        "isPublished": true,
                        "title": "My Article 1",
                        "body": "This is a post",
                        "author": {
                            "id": "123",
                            "name": "John Smith",
                            "pic": {
                                "url": "cdn://123",
                                "width": 640,
                                "height": 480,
                            },
                            "recentArticle": {
                                "id": "1",
                                "isPublished": true,
                                "title": "My Article 1",
                                "body": "This is a post",
                                "keywords": ["foo", "bar", "1", "true", null],
                            },
                        },
                    },
                },
            }),
        );
    }
}
