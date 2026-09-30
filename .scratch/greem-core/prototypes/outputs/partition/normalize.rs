// Compile-pass: normalize repeated arms where their equality is statically known.
enum Either<A, B> { Left(A), Right(B) }
struct As<Tag, T>(T, std::marker::PhantomData<Tag>);
enum User {}
struct BorrowedUser<'a>(&'a str);
fn main() {
    let owned = String::from("borrowed");
    let inputs: Vec<Either<As<User, BorrowedUser<'_>>, As<User, BorrowedUser<'_>>>> = vec![
        Either::Left(As(BorrowedUser(&owned), std::marker::PhantomData)),
        Either::Right(As(BorrowedUser(&owned), std::marker::PhantomData)),
    ];
    let normalized: Vec<As<User, BorrowedUser<'_>>> = inputs.into_iter().map(|v| match v {
        Either::Left(value) | Either::Right(value) => value,
    }).collect();
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[1].0.0, "borrowed");
}
