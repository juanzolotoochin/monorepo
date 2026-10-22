use std::{collections::BTreeSet, env, fs, io::Read, path::Path};

fn compare(left: &Path, right: &Path) {
    let names = |dir: &Path| -> BTreeSet<_> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect()
    };
    assert_eq!(names(left), names(right), "output inventory differs");
    for name in names(left) {
        let a = left.join(&name);
        let b = right.join(&name);
        assert_eq!(a.is_dir(), b.is_dir());
        if a.is_dir() {
            compare(&a, &b);
            continue;
        }
        assert_eq!(
            fs::metadata(&a).unwrap().len(),
            fs::metadata(&b).unwrap().len(),
            "size differs: {name:?}"
        );
        let mut a_file = fs::File::open(&a).unwrap();
        let mut b_file = fs::File::open(&b).unwrap();
        let mut a_buf = vec![0; 1024 * 1024];
        let mut b_buf = vec![0; 1024 * 1024];
        loop {
            let n = a_file.read(&mut a_buf).unwrap();
            if n == 0 {
                break;
            }
            b_file.read_exact(&mut b_buf[..n]).unwrap();
            assert!(a_buf[..n] == b_buf[..n], "contents differ: {name:?}");
        }
    }
}

#[test]
fn model_conversion_is_reproducible() {
    compare(
        Path::new(&env::var("MODEL").unwrap()),
        Path::new(&env::var("MODEL_REPEAT").unwrap()),
    );
    compare(
        Path::new(&env::var("DRAFT").unwrap()),
        Path::new(&env::var("DRAFT_REPEAT").unwrap()),
    );
}
