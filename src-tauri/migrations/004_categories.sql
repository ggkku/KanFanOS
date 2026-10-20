-- 档期来自目录名本身，即使暂时没有任何番剧也应当在侧栏出现。
CREATE TABLE categories (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    sort_key INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (library_id, label)
);

CREATE INDEX idx_categories_library ON categories(library_id, sort_key);
