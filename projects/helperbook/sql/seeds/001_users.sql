INSERT INTO users (id, phone, phone_verified, name, email, email_verified, role, status, account_status, is_admin, referral_code)
VALUES
  ('00000000-0000-0000-0000-000000000001', '+37360000001', true, 'Admin User', 'admin@helperbook.local', true, 'both', 'active', 'active', true, 'ADMIN01'),
  ('00000000-0000-0000-0000-000000000002', '+37360000002', true, 'Test Provider', 'provider@helperbook.local', true, 'provider', 'active', 'active', false, 'PROV02'),
  ('00000000-0000-0000-0000-000000000003', '+37360000003', true, 'Test Client', 'client@helperbook.local', true, 'client', 'active', 'active', false, 'CLIENT03')
ON CONFLICT (id) DO NOTHING;
