//! The two languages must define the same keys with the same placeholders
//! (the Rust counterpart of test/i18n.test.js).

use blink_core::i18n::{keys, Lang};

#[test]
fn english_and_indonesian_define_the_same_keys() {
    let (mut en, mut id) = (keys(Lang::En), keys(Lang::Id));
    en.sort_unstable();
    id.sort_unstable();
    assert_eq!(en, id);
    assert!(en.len() > 150);
}