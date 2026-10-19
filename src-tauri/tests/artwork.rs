//! Integration tests for the cover-art cache.
//!
//! The network providers are disabled: these tests pin the cache contract
//! (keys, TTL, negative caching, normalisation) which is what the UI and the
//! Discord pipeline actually depend on. Provider request/response matching is
//! covered by pure unit tests in `artwork::providers`.

use std::io::Cursor;

use prism_lib::artwork::{ArtworkService, COVER_SIZE, CoverHint};
use prism_lib::db::connect;

/// Write a solid-colour JPEG of the given size.
fn write_jpeg(path: &std::path::Path, width: u32, height: u32) {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    std::fs::write(path, bytes).expect("write jpeg");
}

#[tokio::test]
async fn folder_artwork_is_normalised_and_cached() {
    let temp = tempfile::tempdir().expect("tempdir");
    let album_dir = temp.path().join("Kid A");
    std::fs::create_dir_all(&album_dir).expect("mkdir");
    // A 2400 px scan, larger than the cache bound.
    write_jpeg(&album_dir.join("cover.jpg"), 2_400, 2_400);

    let pool = connect(&temp.path().join("prism.db")).await.expect("db");
    let service = ArtworkService::new(pool.clone(), temp.path().join("artwork"))
        .expect("service")
        .with_offline(true);

    let asset = service
        .ensure(
            "Radiohead",
            "Kid A",
            &CoverHint {
                folder: Some(album_dir.clone()),
                track_path: None,
            },
        )
        .await;

    assert_eq!(asset.provider, "folder");
    assert!(asset.remote_url.is_none(), "local art has no HTTP URL");
    let local = asset.local_path.expect("local path");
    assert!(std::path::Path::new(&local).exists());
    assert!(asset.width.expect("width") <= COVER_SIZE);
    assert!(asset.height.expect("height") <= COVER_SIZE);

    // The cached file is a JPEG and is much smaller than the source.
    let cached = std::fs::read(&local).expect("read cached");
    assert_eq!(&cached[..2], &[0xFF, 0xD8], "JPEG magic");
    let source_size = std::fs::metadata(album_dir.join("cover.jpg"))
        .expect("source")
        .len();
    assert!((cached.len() as u64) < source_size);

    // A second call is served from SQLite without touching the disk image.
    std::fs::remove_file(album_dir.join("cover.jpg")).expect("remove source");
    let again = service
        .ensure(
            "Radiohead",
            "Kid A",
            &CoverHint {
                folder: Some(album_dir.clone()),
                track_path: None,
            },
        )
        .await;
    assert_eq!(again.provider, "folder");
    assert_eq!(again.local_path.as_deref(), Some(local.as_str()));

    // The cache row carries the SHA-256 identity and the provider CHECK value.
    let (provider, artist_key, album_key): (String, String, String) =
        sqlx::query_as("SELECT provider, artist_key, album_key FROM cover_cache WHERE id = ?1")
            .bind(&asset.key)
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(provider, "folder");
    assert_eq!(artist_key, "radiohead");
    assert_eq!(album_key, "kid a");
    assert_eq!(asset.key, ArtworkService::key("radiohead", "  KID A "));

    pool.close().await;
}

#[tokio::test]
async fn a_missing_cover_is_negatively_cached() {
    let temp = tempfile::tempdir().expect("tempdir");
    let pool = connect(&temp.path().join("prism.db")).await.expect("db");
    let service = ArtworkService::new(pool.clone(), temp.path().join("artwork"))
        .expect("service")
        .with_offline(true);

    let asset = service
        .ensure("Nobody", "Nothing", &CoverHint::default())
        .await;
    assert!(asset.is_placeholder());
    assert!(asset.local_path.is_none());
    assert!(asset.remote_url.is_none());

    // The negative row exists, is a placeholder and expires within a day.
    let (provider, remote, attempts, fresh): (String, Option<String>, i64, i64) = sqlx::query_as(
        "SELECT provider, remote_url, attempts, \
                CASE WHEN expires_at > strftime('%Y-%m-%dT%H:%M:%fZ','now') THEN 1 ELSE 0 END \
         FROM cover_cache WHERE id = ?1",
    )
    .bind(&asset.key)
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(provider, "unknown");
    assert_eq!(remote.as_deref(), Some(""));
    assert_eq!(attempts, 1);
    assert_eq!(fresh, 1);

    // A second lookup re-runs the providers (incrementing attempts) instead of
    // hammering every caller — and still returns a placeholder.
    let again = service
        .ensure("Nobody", "Nothing", &CoverHint::default())
        .await;
    assert!(again.is_placeholder());
    let attempts: i64 = sqlx::query_scalar("SELECT attempts FROM cover_cache WHERE id = ?1")
        .bind(&asset.key)
        .fetch_one(&pool)
        .await
        .expect("attempts");
    assert_eq!(attempts, 2);

    // The sweep removes the expired placeholder.
    sqlx::query("UPDATE cover_cache SET expires_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1")
        .bind(&asset.key)
        .execute(&pool)
        .await
        .expect("expire");
    assert_eq!(service.sweep().await.expect("sweep"), 1);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cover_cache")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(left, 0);

    pool.close().await;
}

#[tokio::test]
async fn embedded_artwork_wins_over_the_folder() {
    use lofty::config::WriteOptions;
    use lofty::picture::{Picture, PictureType};
    use lofty::prelude::{Accessor, TagExt};
    use lofty::tag::{Tag, TagType};

    let temp = tempfile::tempdir().expect("tempdir");
    let album_dir = temp.path().join("Amnesiac");
    std::fs::create_dir_all(&album_dir).expect("mkdir");
    write_jpeg(&album_dir.join("cover.jpg"), 600, 600);

    // A 1000 px PNG embedded in a WAV, tagged through lofty exactly as a
    // well-curated rip would be.
    let track = album_dir.join("01 Packt Like Sardines.wav");
    let mut pcm = Vec::new();
    let frames = 44_100u32;
    let block = 4u32;
    let data_len = frames * block;
    pcm.extend_from_slice(b"RIFF");
    pcm.extend_from_slice(&(36 + data_len).to_le_bytes());
    pcm.extend_from_slice(b"WAVEfmt ");
    pcm.extend_from_slice(&16u32.to_le_bytes());
    pcm.extend_from_slice(&1u16.to_le_bytes());
    pcm.extend_from_slice(&2u16.to_le_bytes());
    pcm.extend_from_slice(&44_100u32.to_le_bytes());
    pcm.extend_from_slice(&(44_100 * block).to_le_bytes());
    pcm.extend_from_slice(&4u16.to_le_bytes());
    pcm.extend_from_slice(&16u16.to_le_bytes());
    pcm.extend_from_slice(b"data");
    pcm.extend_from_slice(&data_len.to_le_bytes());
    pcm.resize(pcm.len() + data_len as usize, 0);
    std::fs::write(&track, pcm).expect("write wav");

    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(1_000, 1_000, |x, y| {
        image::Rgb([(x % 256) as u8, 0, (y % 256) as u8])
    }))
    .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
    .expect("encode png");
    let picture = Picture::new_unchecked(PictureType::CoverFront, None, None, png);

    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Packt Like Sardines in a Crushd Tin Box".to_string());
    tag.push_picture(picture);
    tag.save_to_path(&track, WriteOptions::default())
        .expect("write tag");

    let pool = connect(&temp.path().join("prism.db")).await.expect("db");
    let service = ArtworkService::new(pool.clone(), temp.path().join("artwork"))
        .expect("service")
        .with_offline(true);

    let asset = service
        .ensure(
            "Radiohead",
            "Amnesiac",
            &CoverHint {
                folder: Some(album_dir.clone()),
                track_path: Some(track),
            },
        )
        .await;
    assert_eq!(
        asset.provider, "embedded",
        "embedded art outranks the folder"
    );
    assert!(asset.local_path.is_some());
    assert_eq!(asset.width, Some(COVER_SIZE), "1:1 source scales exactly");

    pool.close().await;
}
