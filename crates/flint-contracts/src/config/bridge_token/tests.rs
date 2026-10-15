use super::load_or_create_at;
use std::{fs, thread};
use uuid::Uuid;

#[test]
fn concurrent_creators_agree_on_one_token() {
    let directory = std::env::temp_dir().join(format!("flint-token-{}", Uuid::new_v4().simple()));
    let path = directory.join("bridge-token");
    let tokens: Vec<String> = thread::scope(|scope| {
        let creators: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| load_or_create_at(&path).unwrap()))
            .collect();
        creators.into_iter().map(|creator| creator.join().unwrap()).collect()
    });
    fs::remove_dir_all(&directory).unwrap();
    assert!(!tokens[0].is_empty());
    assert!(tokens.iter().all(|token| *token == tokens[0]));
}
