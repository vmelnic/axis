INSERT INTO app_versions (id, version, platform, min_supported, recommended, force_update)
VALUES
  ('00000000-0000-0000-0000-200000000001', '1.0.0', 'ios', '1.0.0', '1.0.0', false),
  ('00000000-0000-0000-0000-200000000002', '1.0.0', 'android', '1.0.0', '1.0.0', false),
  ('00000000-0000-0000-0000-200000000003', '1.0.0', 'web', '1.0.0', '1.0.0', false)
ON CONFLICT (id) DO NOTHING;
