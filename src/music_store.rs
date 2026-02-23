use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Error, anyhow};
use lofty::file::TaggedFileExt;
use lofty::read_from_path;
use lofty::tag::Accessor;
use lofty::tag::Tag;
use rusqlite::Connection;
use rusqlite::Result;
use tracing::debug;
use tracing::error;

#[derive(Debug, Clone)]
pub struct MusicItem {
    pub id: i32,
    pub name: String,
    pub cover: String,
}

#[derive(Debug, Clone)]
pub struct MusicTitleItem {
    pub id: i32,
    pub name: String,
    pub path: String,
    pub cover: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone)]
pub struct KidsAlbumItem {
    pub id: i32,
    pub album_name: String,
    pub artist_name: String,
    pub cover: String,
}

#[derive(Debug)]
pub struct NewMusicTitle {
    pub name: String,
    pub path: String,
    pub cover: String,
    pub artist: String,
    pub album: String,
    pub source: String,
    pub track: u32,
}

pub struct MusicStore {
    db_connection: Arc<Mutex<Connection>>,
}

unsafe impl Send for MusicStore {}

impl MusicStore {
    pub fn new(db_connection: Connection) -> Self {
        Self {
            db_connection: Arc::new(Mutex::new(db_connection)),
        }
    }

    pub fn init(&self) -> Result<()> {
        let db_connection = self.db_connection.lock().expect("DB is locked");

        db_connection.execute(
            "CREATE TABLE IF NOT EXISTS sources (
                    id     INTEGER PRIMARY KEY,
                    source TEXT NOT NULL,
                    UNIQUE(source) ON CONFLICT IGNORE
                )",
            (), // empty list of parameters.
        )?;

        db_connection.execute(
            "CREATE TABLE IF NOT EXISTS covers (
                    id     INTEGER PRIMARY KEY,
                    source INTEGER NOT NULL,
                    path   TEXT NOT NULL,
                    FOREIGN KEY (source) REFERENCES sources(id),
                    UNIQUE(path) ON CONFLICT IGNORE
                )",
            (), // empty list of parameters.
        )?;

        db_connection.execute(
            "CREATE TABLE IF NOT EXISTS artists (
                    id     INTEGER PRIMARY KEY,
                    source INTEGER NOT NULL,
                    artist TEXT NOT NULL,
                    cover  INTEGER,
                    FOREIGN KEY (source) REFERENCES sources(id),
                    FOREIGN KEY (cover) REFERENCES covers(id)
                    UNIQUE(artist) ON CONFLICT IGNORE
                )",
            (), // empty list of parameters.
        )?;
        db_connection.execute(
            "CREATE TABLE IF NOT EXISTS albums (
                    id     INTEGER PRIMARY KEY,
                    source INTEGER NOT NULL,
                    artist INTEGER NOT NULL,
                    album  TEXT NOT NULL,
                    cover  INTEGER,
                    FOREIGN KEY (source) REFERENCES sources(id),
                    FOREIGN KEY (artist) REFERENCES artists(id),
                    FOREIGN KEY (cover) REFERENCES covers(id),
                    UNIQUE(artist, album) ON CONFLICT IGNORE
                )",
            (), // empty list of parameters.
        )?;
        db_connection.execute(
            "CREATE TABLE IF NOT EXISTS titles (
                    id     INTEGER PRIMARY KEY,
                    source INTEGER NOT NULL,
                    artist INTEGER NOT NULL,
                    album  INTEGER NOT NULL,
                    title  TEXT NOT NULL,
                    path   TEXT UNIQUE NOT NULL,
                    cover  INTEGER,
                    track  INTEGER NOT NULL,
                    FOREIGN KEY (source) REFERENCES sources(id),
                    FOREIGN KEY (artist) REFERENCES artists(id),
                    FOREIGN KEY (album) REFERENCES albums(id),
                    FOREIGN KEY (cover) REFERENCES covers(id),
                    UNIQUE(artist, album, title) ON CONFLICT IGNORE
                )",
            (), // empty list of parameters.
        )?;
        drop(db_connection);

        Ok(())
    }

    pub fn add_title(&self, title: &NewMusicTitle) -> Result<()> {
        // debug!(
        //     "Add title: {} {} {} {} {} {} {}",
        //     title.source,
        //     title.artist,
        //     title.album,
        //     title.name,
        //     title.path,
        //     title.cover,
        //     title.track
        // );
        let db_connection = self.db_connection.lock().expect("DB is locked");
        db_connection.execute("INSERT INTO sources (source) VALUES (?1)", [&title.source])?;
        drop(db_connection);
        let source_id = self.get_source_id(&title.source)?;
        let db_connection = self.db_connection.lock().expect("DB is locked");
        db_connection.execute(
            "INSERT INTO covers (source, path) VALUES (?1, ?2)",
            [&source_id.to_string(), &title.cover],
        )?;
        drop(db_connection);
        let cover_id = self.get_cover_id(&title.cover)?;
        let db_connection = self.db_connection.lock().expect("DB is locked");
        debug!("Insert Artist {}", title.artist);
        db_connection.execute(
            "INSERT INTO artists (source, artist, cover) VALUES (?1, ?2, ?3)",
            [&source_id.to_string(), &title.artist, &cover_id.to_string()],
        )?;
        drop(db_connection);
        let artist_id = self.get_artist_id(&title.artist)?;
        debug!("Artist has ID: {artist_id}");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        db_connection.execute(
            "INSERT INTO albums (source, artist, album, cover) VALUES (?1, ?2, ?3, ?4)",
            [
                &source_id.to_string(),
                &artist_id.to_string(),
                &title.album,
                &cover_id.to_string(),
            ],
        )?;
        drop(db_connection);
        let album_id = self.get_album_id(&title.album)?;
        let db_connection = self.db_connection.lock().expect("DB is locked");
        db_connection.execute(
            "INSERT INTO titles (source, artist, album, title, path, cover, track) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            [
                &source_id.to_string(),
                &artist_id.to_string(),
                &album_id.to_string(),
                &title.name,
                &title.path,
                &cover_id.to_string(),
                &title.track.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn get_source_id(&self, source: &String) -> Result<i32> {
        debug!("Get osurce id");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare("SELECT id FROM sources WHERE source=(?1)")?;
        stmt.query_row([source], |row| Ok(row.get(0)))?
    }

    pub fn get_cover_id(&self, path: &String) -> Result<i32> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare("SELECT id FROM covers WHERE path=(?1)")?;
        stmt.query_row([path], |row| Ok(row.get(0)))?
    }

    pub fn get_artists(&self, source: i32) -> Result<Vec<MusicItem>> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT artists.id,artists.artist,covers.path FROM artists INNER JOIN covers ON artists.cover=covers.id WHERE artists.source=(?1) ORDER BY artists.artist")?;
        let rows = stmt.query_map([source], |row| {
            Ok(MusicItem {
                id: row.get(0)?,
                name: row.get(1)?,
                cover: row.get(2)?,
            })
        })?;
        let mut artists: Vec<MusicItem> = vec![];
        for row in rows {
            match row {
                Ok(artist) => artists.push(artist),
                Err(_) => (),
            }
        }
        Ok(artists)
    }

    pub fn get_artist_id(&self, artist: &String) -> Result<i32> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare("SELECT id FROM artists WHERE artist=?1")?;
        stmt.query_row([artist], |row| Ok(row.get(0)))?
    }

    pub fn get_albums(&self, source: i32) -> Result<Vec<MusicItem>> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT albums.id,albums.album,covers.path FROM albums INNER JOIN covers ON albums.cover=covers.id WHERE albums.source=(?1) ORDER BY albums.album")?;
        let rows = stmt.query_map([source], |row| {
            Ok(MusicItem {
                id: row.get(0)?,
                name: row.get(1)?,
                cover: row.get(2)?,
            })
        })?;
        let mut albums: Vec<MusicItem> = vec![];
        for row in rows {
            match row {
                Ok(album) => albums.push(album),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(albums)
    }

    pub fn get_album_id(&self, album: &String) -> Result<i32> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare("SELECT id FROM albums WHERE album=(?1)")?;
        stmt.query_row([album], |row| Ok(row.get(0)))?
    }

    pub fn get_albums_with_artist(&self, source: i32) -> Result<Vec<KidsAlbumItem>> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare(
            "SELECT albums.id, albums.album, artists.artist, covers.path \
             FROM albums \
             INNER JOIN artists ON albums.artist = artists.id \
             INNER JOIN covers ON albums.cover = covers.id \
             WHERE albums.source = (?1) \
             ORDER BY albums.album",
        )?;
        let rows = stmt.query_map([source], |row| {
            Ok(KidsAlbumItem {
                id: row.get(0)?,
                album_name: row.get(1)?,
                artist_name: row.get(2)?,
                cover: row.get(3)?,
            })
        })?;
        let mut albums: Vec<KidsAlbumItem> = vec![];
        for row in rows {
            match row {
                Ok(album) => albums.push(album),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(albums)
    }

    pub fn get_albums_by_artist(&self, source: i32, artist_id: i32) -> Result<Vec<MusicItem>> {
        debug!("Get albums by artist {artist_id} and source {source} ...");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection.prepare(
            "SELECT albums.id,albums.album,covers.path FROM albums INNER JOIN covers ON albums.cover=covers.id WHERE albums.source=(?1) AND albums.artist=(?2) ORDER BY albums.album",
        )?;
        let rows = stmt.query_map([source, artist_id], |row| {
            Ok(MusicItem {
                id: row.get(0)?,
                name: row.get(1)?,
                cover: row.get(2)?,
            })
        })?;
        let mut albums: Vec<MusicItem> = vec![];
        for row in rows {
            match row {
                Ok(album) => albums.push(album),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(albums)
    }

    pub fn get_titles(&self, source: i32) -> Result<Vec<MusicTitleItem>> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT titles.id,titles.title,titles.path,covers.path,artists.artist,albums.album FROM titles INNER JOIN artists ON titles.artist=artists.id, albums ON titles.album=albums.id, covers ON titles.cover=covers.id WHERE titles.source=(?1) ORDER BY titles.artist,titles.album,titles.track")?;
        let rows = stmt.query_map([source], |row| {
            Ok(MusicTitleItem {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                cover: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })?;
        let mut titles: Vec<MusicTitleItem> = vec![];
        for row in rows {
            match row {
                Ok(title) => titles.push(title),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(titles)
    }

    pub fn get_titles_by_artist(&self, source: i32, artist: i32) -> Result<Vec<MusicTitleItem>> {
        debug!("Get titles by artist {artist} ...");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT titles.id,titles.title,titles.path,covers.path,artists.artist,albums.album FROM titles INNER JOIN artists ON titles.artist=artists.id, albums ON titles.album=albums.id, covers ON titles.cover=covers.id WHERE titles.source=(?1) AND titles.artist=(?2) ORDER BY titles.album,titles.track")?;
        let rows = stmt.query_map([source, artist], |row| {
            Ok(MusicTitleItem {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                cover: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })?;
        let mut titles: Vec<MusicTitleItem> = vec![];
        for row in rows {
            match row {
                Ok(title) => titles.push(title),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(titles)
    }

    pub fn get_titles_by_album(&self, source: i32, album: i32) -> Result<Vec<MusicTitleItem>> {
        debug!("Get titles by album {album} and source {source} ...");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT titles.id,titles.title,titles.path,covers.path,artists.artist,albums.album FROM titles INNER JOIN artists ON titles.artist=artists.id, albums ON titles.album=albums.id, covers ON titles.cover=covers.id WHERE titles.source=(?1) AND titles.album=(?2) ORDER BY titles.artist,titles.track")?;
        let rows = stmt.query_map([source, album], |row| {
            Ok(MusicTitleItem {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                cover: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })?;
        let mut titles: Vec<MusicTitleItem> = vec![];
        for row in rows {
            match row {
                Ok(title) => titles.push(title),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(titles)
    }

    pub fn get_titles_by_artist_and_album(
        &self,
        source: i32,
        artist: i32,
        album: i32,
    ) -> Result<Vec<MusicTitleItem>> {
        debug!("Get titles by artist {artist}, album {album} and source {source} ...");
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT titles.id,titles.title,titles.path,covers.path,artists.artist,albums.album FROM titles INNER JOIN artists ON titles.artist=artists.id, albums ON titles.album=albums.id, covers ON titles.cover=covers.id WHERE titles.source = (?1) AND titles.artist=(?2) AND titles.album=(?3) ORDER BY titles.track")?;
        let rows = stmt.query_map([source, artist, album], |row| {
            Ok(MusicTitleItem {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                cover: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })?;
        let mut titles: Vec<MusicTitleItem> = vec![];
        for row in rows {
            match row {
                Ok(title) => titles.push(title),
                Err(e) => error!("Error: {}", e),
            }
        }
        Ok(titles)
    }

    pub fn get_title_by_id(&self, id: i32) -> Result<MusicTitleItem> {
        let db_connection = self.db_connection.lock().expect("DB is locked");
        let mut stmt = db_connection
            .prepare("SELECT titles.id,titles.title,titles.path,covers.path,artists.artist,albums.album FROM titles INNER JOIN artists ON titles.artist=artists.id, albums ON titles.album=albums.id, covers ON titles.cover=covers.id WHERE titles.id=(?1)")?;
        stmt.query_row([id], |row| {
            Ok(MusicTitleItem {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                cover: row.get(3)?,
                artist: row.get(4)?,
                album: row.get(5)?,
            })
        })
    }

    pub fn update(&self, source_name: &String, path: &String) -> Result<(), Error> {
        if let Ok(source_id) = self.get_source_id(source_name) {
            debug!("Source ID is {source_id}");
            let db_connection = self.db_connection.lock().expect("DB is locked");
            db_connection.execute("DELETE FROM titles WHERE source=(?1)", [&source_id])?;
            db_connection.execute("DELETE FROM albums WHERE source=(?1)", [&source_id])?;
            db_connection.execute("DELETE FROM artists WHERE source=(?1)", [&source_id])?;
            db_connection.execute("DELETE FROM covers WHERE source=(?1)", [&source_id])?;
            db_connection.execute("DELETE FROM sources WHERE source=(?1)", [&source_id])?;
            drop(db_connection);
        }

        self.incremental_update(source_name, path)
    }

    pub fn incremental_update(&self, source_name: &String, path: &String) -> Result<(), Error> {
        debug!("Update source {source_name} with path {path} ...");

        match fs::exists(path) {
            Ok(exists) => {
                if !exists {
                    return Err(anyhow!("Path {path} does not exist, abort."));
                }
            }
            Err(error) => return Err(anyhow!("Could not read path {path}: {error}")),
        }
        let paths: Vec<PathBuf> = fs::read_dir(path)?
            .filter_map(Result::ok)
            .map(|file| file.path())
            .collect();
        let mut files: Vec<PathBuf> = vec![];
        let mut images: Vec<PathBuf> = vec![];
        for path in paths {
            match path {
                f if f.is_dir() => self.incremental_update(
                    source_name,
                    &f.to_str()
                        .ok_or(anyhow!("Could not get String from file name"))?
                        .to_string(),
                )?,
                f if f.is_file() => {
                    if MusicStore::is_supported_extension(&f) {
                        files.push(f)
                    } else if MusicStore::is_supported_cover_extension(&f) {
                        images.push(f)
                    }
                }
                _ => (),
            }
        }
        let cover = MusicStore::get_cover(images);
        files.sort();
        files
            .iter()
            .enumerate()
            .map(|(i, file)| (file, MusicStore::get_metadata(file, (i + 1) as u32)))
            .for_each(|(file, result)| match result {
                Ok(tag) => {
                    let artist = tag
                        .artist()
                        .unwrap_or(std::borrow::Cow::Borrowed("Unknown Artist"));
                    let album = tag
                        .album()
                        .unwrap_or(std::borrow::Cow::Borrowed("Unknown Album"));
                    let title = tag
                        .title()
                        .unwrap_or(std::borrow::Cow::Borrowed("Unknown Title"));
                    let track = tag.track().unwrap_or(0);
                    let add_result = self.add_title(&NewMusicTitle {
                        source: source_name.to_string(),
                        artist: artist.to_string(),
                        album: album.to_string(),
                        name: title.to_string(),
                        path: file
                            .to_str()
                            .expect("Could not get String from file name")
                            .to_string(),
                        cover: cover
                            .to_str()
                            .expect("Could not get String from file name")
                            .to_string(),
                        track,
                    });
                    match add_result {
                        Ok(_) => (),
                        Err(error) => error!("Could not add title to DB: {error}"),
                    };
                }
                Err(e) => error!("Error: {}", e),
            });
        Ok(())
    }

    fn is_supported_extension(path: &Path) -> bool {
        matches!(
            path.extension()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .to_str(),
            Some("mp3") | Some("flac")
        )
    }

    fn is_supported_cover_extension(path: &Path) -> bool {
        matches!(
            path.extension()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .to_str(),
            Some("jpg") | Some("jpeg") | Some("png")
        )
    }

    fn get_cover(paths: Vec<PathBuf>) -> PathBuf {
        match paths.len() {
            l if l > 1 => {
                let mut cover = paths.first().unwrap().clone();
                for path in paths {
                    let path_str = path.to_str().unwrap();
                    if path_str.contains("cover") || path_str.contains("front") {
                        cover = path.clone();
                    }
                }
                cover
            }
            1 => paths[0].clone(),
            _ => Path::new("images/no_image.jpg").to_path_buf(),
        }
    }

    fn get_metadata(path: &PathBuf, track: u32) -> Result<Tag, Error> {
        let file = read_from_path(path)?;
        let mut tag = file
            .primary_tag()
            .ok_or(anyhow!(
                "Could not find primary tag in metadata of file {}",
                path.as_os_str().to_str().unwrap()
            ))?
            .clone();
        if tag.track().is_none() {
            tag.set_track(track);
        }
        Ok(tag)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_supported_extension_mp3() {
        let path = Path::new("test.mp3").to_path_buf();
        assert!(MusicStore::is_supported_extension(&path));
    }

    #[test]
    fn test_is_supported_extension_flac() {
        let path = Path::new("test.flac").to_path_buf();
        assert!(MusicStore::is_supported_extension(&path));
    }

    #[test]
    fn test_is_supported_extension_not_supported() {
        let path = Path::new("test.wav").to_path_buf();
        assert!(!MusicStore::is_supported_extension(&path));
    }

    #[test]
    fn test_is_supported_cover_extension_jpg() {
        let path = Path::new("test.jPg").to_path_buf();
        assert!(MusicStore::is_supported_cover_extension(&path));
    }

    #[test]
    fn test_is_supported_cover_extension_jpeg() {
        let path = Path::new("test.Jpeg").to_path_buf();
        assert!(MusicStore::is_supported_cover_extension(&path));
    }

    #[test]
    fn test_is_supported_cover_extension_png() {
        let path = Path::new("test.png").to_path_buf();
        assert!(MusicStore::is_supported_cover_extension(&path));
    }

    #[test]
    fn test_is_supported_cover_extension_not_supported() {
        let path = Path::new("test.gif").to_path_buf();
        assert!(!MusicStore::is_supported_cover_extension(&path));
    }

    #[test]
    fn test_get_cover_two_no_match_cover() {
        let paths = vec![PathBuf::from("test.jpg"), PathBuf::from("test.png")];
        assert_eq!(MusicStore::get_cover(paths), PathBuf::from("test.jpg"));
    }

    #[test]
    fn test_get_cover_two_match_cover() {
        let paths = vec![PathBuf::from("test.jpg"), PathBuf::from("cover.png")];
        assert_eq!(MusicStore::get_cover(paths), PathBuf::from("cover.png"));
    }

    #[test]
    fn test_get_cover_two_match_front() {
        let paths = vec![PathBuf::from("test.jpg"), PathBuf::from("front.jpg")];
        assert_eq!(MusicStore::get_cover(paths), PathBuf::from("front.jpg"));
    }

    #[test]
    fn test_get_cover_one() {
        let paths = vec![PathBuf::from("test.jpg")];
        assert_eq!(MusicStore::get_cover(paths), PathBuf::from("test.jpg"));
    }

    #[test]
    fn test_get_cover_none() {
        let paths = vec![];
        assert_eq!(
            MusicStore::get_cover(paths),
            PathBuf::from("images/no_image.jpg")
        );
    }

    #[test]
    fn test_create_db() -> Result<()> {
        let test_db = rusqlite::Connection::open("./test_db.db3")?;
        let music_store = MusicStore::new(test_db);
        music_store.init()?;
        let test_db = rusqlite::Connection::open("./test_db.db3")?;
        let mut stmt = test_db.prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY 1")?;
        let rows = stmt.query_map([], |row| Ok(row.get::<usize, String>(0).unwrap()))?;
        let tables: Vec<String> = rows.map(|row| row.unwrap()).collect();
        assert!(tables.contains(&"albums".to_string()));
        assert!(tables.contains(&"artists".to_string()));
        assert!(tables.contains(&"sources".to_string()));
        assert!(tables.contains(&"titles".to_string()));
        let _ = std::fs::remove_file("./test_db.db3");
        Ok(())
    }
}
