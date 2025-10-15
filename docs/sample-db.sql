CREATE TABLE users (
  id SERIAL PRIMARY KEY,
  email text NOT NULL,
  name text NOT NULL
);

CREATE TABLE blogs (
  id SERIAL PRIMARY KEY,
  name text NOT NULL,
  posts jsonb NOT NULL
);

CREATE TABLE user_blogs (
  user_id SERIAL NOT NULL REFERENCES users(id),
  blog_id SERIAL NOT NULL REFERENCES blogs(id),
  role TEXT NOT NULL
);

CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    content text NOT NULL
);

INSERT INTO posts (content) VALUES
  ('Charlie''s first post'),
  ('I went on holiday'),
  ('Presenting our new product'),
  ('Introducing the team');

INSERT INTO blogs (name, posts) VALUES
  ('Charlie''s blog', '[{"postId": 1}, {"postId": 2}]'),
  ('Example Inc. blog', '[{"postId": 3}, {"postId": 4}]'),
  ('The blog of Alice', '[]');

INSERT INTO users (email, name) VALUES
  ('alice.johnson@example.com', 'Alice Johnson'),
  ('bob.smith@example.com', 'Bob Smith'),
  ('charlie.brown@example.com', 'Charlie Brown'),
  ('diana.miller@example.com', 'Diana Miller'),
  ('edward.wilson@example.com', 'Edward Wilson'),
  ('fiona.garcia@example.com', 'Fiona Garcia'),
  ('george.taylor@example.com', 'George Taylor'),
  ('hannah.lee@example.com', 'Hannah Lee'),
  ('ian.clark@example.com', 'Ian Clark'),
  ('julia.martinez@example.com', 'Julia Martinez');

INSERT INTO user_blogs VALUES (3, 1, 'editor'), (3, 2, 'editor'), (3, 3, 'reader');
