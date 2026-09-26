#![allow(unexpected_cfgs)]
use futures::future::{BoxFuture, join_all};
use serde::Serialize;
#[derive(Serialize)]
struct Response<'a> { data: &'a [&'a str] }
trait Scope: Send + Sync {
    fn run(&self) -> BoxFuture<'_, Vec<Box<dyn Scope + '_>>>;
    fn value(&self) -> &str;
}
struct Root { value: String }
struct Child<'a> { value: &'a str }
impl Scope for Root {
    fn run(&self) -> BoxFuture<'_, Vec<Box<dyn Scope + '_>>> {
        Box::pin(async move { vec![Box::new(Child { value: &self.value }) as Box<dyn Scope>] })
    }
    fn value(&self) -> &str { &self.value }
}
impl Scope for Child<'_> {
    fn run(&self) -> BoxFuture<'_, Vec<Box<dyn Scope + '_>>> { Box::pin(async { vec![] }) }
    fn value(&self) -> &str { self.value }
}
fn execute<'a, R: Send + 'a, F>(generation: Vec<Box<dyn Scope + 'a>>, values: Vec<&'a str>, finish: F) -> BoxFuture<'a, R>
where F: for<'r> FnOnce(Response<'r>) -> R + Send + 'a {
    Box::pin(async move {
        let next: Vec<_> = join_all(generation.iter().map(|s| s.run())).await.into_iter().flatten().collect();
        let values: Vec<_> = values.into_iter().chain(generation.iter().map(|s| s.value())).collect();
        if next.is_empty() { finish(Response { data: &values }) }
        else { execute(next, values, finish).await }
    })
}
fn main() {
    let roots: Vec<Box<dyn Scope>> = vec![Box::new(Root { value: "borrowed parent".to_owned() })];
    #[cfg(not(escape))]
    {
        let future = execute(roots, vec![], |response| serde_json::to_string(&response).unwrap());
        let json = futures::executor::block_on(future);
        assert_eq!(json, r#"{"data":["borrowed parent","borrowed parent"]}"#);
        println!("{json}");
    }
    #[cfg(escape)]
    {
        let invalid = execute(roots, vec![], |response| response.data[0]);
        println!("{}", futures::executor::block_on(invalid));
    }
}
