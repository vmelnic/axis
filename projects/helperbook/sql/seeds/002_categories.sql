INSERT INTO service_categories (id, name, slug)
VALUES
  ('00000000-0000-0000-0000-100000000001', 'Cleaning', 'cleaning'),
  ('00000000-0000-0000-0000-100000000002', 'Plumbing', 'plumbing'),
  ('00000000-0000-0000-0000-100000000003', 'Electrical', 'electrical'),
  ('00000000-0000-0000-0000-100000000004', 'Tutoring', 'tutoring'),
  ('00000000-0000-0000-0000-100000000005', 'Beauty', 'beauty')
ON CONFLICT (id) DO NOTHING;
