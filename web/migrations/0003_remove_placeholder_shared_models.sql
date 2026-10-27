-- Migration 0003: remove the shared models seeded by 0001.
--
-- Their sha256 values do not hash their source: 'e3b0c442…' is the SHA-256 of the empty
-- string and '5a4b3c2d…' is a sequential pattern. Their source code also differs from the
-- playground models. Links to /p/quadcopter and /p/server_capacity keep working: the
-- playground recognises those names without looking them up.

DELETE FROM shared_models WHERE (id, sha256) IN (
    ('quadcopter',      'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'),
    ('server_capacity', '5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b')
);
