include!("common_inc.rs");

#[tokio::test]
async fn read_with_this_lore() {
    let Some(root) = std::env::var_os("LORE_DOWNGRADE_DIR") else { return };
    let root = std::path::PathBuf::from(root);
    let immutable = match LocalImmutableStore::new(Some(root.clone()), ImmutableStoreSettings::default()).await {
        Ok(store) => store,
        Err(err) => {
            println!("READER the immutable store refused to open: {err}");
            return;
        }
    };
    let dyn_immutable: Arc<dyn ImmutableStore> = immutable.clone();
    let mutable = match LocalMutableStore::new(Some(&root), MutableStoreSettings::default(), dyn_immutable).await {
        Ok(store) => Arc::new(store),
        Err(err) => {
            println!("READER the mutable store refused to open: {err}");
            return;
        }
    };
    let mut found = 0;
    for n in 0..COUNT {
        if immutable.clone().get(partition(), address(n)).await.is_ok_and(|got| got.payload.is_some()) {
            found += 1;
        }
    }
    let tip = mutable.clone().load(partition(), key(), KeyType::Untyped).await;
    println!("READER opened: {found} of {COUNT} fragments read back; the key reads {:?} (wanted {})", tip.as_ref().map(|h| h.to_string()), value());
}
