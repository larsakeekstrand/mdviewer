-- A small SQL example: a simple blog schema with a query.

CREATE TABLE authors (
    id   INTEGER PRIMARY KEY,
    name TEXT    NOT NULL,
    bio  TEXT
);

CREATE TABLE posts (
    id        INTEGER PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES authors(id),
    title     TEXT    NOT NULL,
    published BOOLEAN NOT NULL DEFAULT FALSE,
    created   DATE    NOT NULL
);

INSERT INTO authors (id, name) VALUES (1, 'Alice'), (2, 'Bob');

INSERT INTO posts (id, author_id, title, published, created) VALUES
    (1, 1, 'Hello World',    TRUE,  '2024-01-10'),
    (2, 1, 'Draft Post',     FALSE, '2024-03-05'),
    (3, 2, 'Getting Started',TRUE,  '2024-02-14');

-- Fetch published posts with their author name, newest first.
SELECT p.title, a.name AS author, p.created
FROM   posts p
JOIN   authors a ON a.id = p.author_id
WHERE  p.published = TRUE
ORDER  BY p.created DESC;
