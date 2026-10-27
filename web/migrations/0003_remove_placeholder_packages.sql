-- Migration 0003: remove the placeholder packages seeded by 0001 and 0002.
--
-- Those rows were never packed with `avenc pkg pack`: their sha256_seal values are
-- sequential hex patterns rather than archive digests, and their download counts were
-- made up. Deleting by name and seal leaves any real package with the same name alone.

DELETE FROM packages WHERE (name, sha256_seal) IN (
    ('std_aerodynamics',    '7a8b9c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b'),
    ('std_thermodynamics',  '8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c'),
    ('fin_markowitz',       '9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d'),
    ('robotics_kinematics', '0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e'),
    ('ind_openplc',         '1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f')
);
