CREATE TABLE IF NOT EXISTS app_versions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  version VARCHAR(20) NOT NULL,
  platform VARCHAR(50) CHECK (platform IN ('ios', 'android', 'web')) NOT NULL,
  min_supported VARCHAR(20) NOT NULL,
  recommended VARCHAR(20) NOT NULL,
  force_update BOOLEAN DEFAULT FALSE,
  release_notes TEXT,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_app_versions_platform ON app_versions (platform);
CREATE INDEX IF NOT EXISTS idx_app_versions_version ON app_versions (version);

CREATE TABLE IF NOT EXISTS deep_links (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  slug VARCHAR(100) NOT NULL UNIQUE,
  type VARCHAR(50) CHECK (type IN ('profile', 'invite', 'chat', 'referral')) NOT NULL,
  target_id UUID,
  metadata TEXT,
  clicks INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_deep_links_slug ON deep_links (slug);
CREATE INDEX IF NOT EXISTS idx_deep_links_type ON deep_links (type);

CREATE TABLE IF NOT EXISTS users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  phone VARCHAR(20) NOT NULL UNIQUE,
  phone_verified BOOLEAN DEFAULT FALSE,
  name VARCHAR(100),
  avatar_url VARCHAR(500),
  bio TEXT,
  location_lat NUMERIC(10,7),
  location_lng NUMERIC(10,7),
  location_name VARCHAR(255),
  role VARCHAR(50) CHECK (role IN ('client', 'provider', 'both')) DEFAULT 'client',
  slug VARCHAR(30) UNIQUE,
  is_provider BOOLEAN DEFAULT FALSE,
  is_admin BOOLEAN DEFAULT FALSE,
  status VARCHAR(50) CHECK (status IN ('active', 'suspended', 'deleted')) DEFAULT 'active',
  account_status VARCHAR(50) CHECK (account_status IN ('active', 'warned', 'suspended', 'banned')) DEFAULT 'active',
  badge_type VARCHAR(50) CHECK (badge_type IN ('id_check', 'verified')),
  badge_status VARCHAR(50) CHECK (badge_status IN ('pending', 'approved', 'rejected')),
  badge_requested_at TIMESTAMPTZ,
  badge_approved_at TIMESTAMPTZ,
  email VARCHAR(255) UNIQUE,
  email_verified BOOLEAN DEFAULT FALSE,
  totp_secret VARCHAR(255),
  totp_enabled BOOLEAN DEFAULT FALSE,
  referral_code VARCHAR(50) UNIQUE,
  last_seen TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_phone ON users (phone);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_slug ON users (slug);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users (email);
CREATE INDEX IF NOT EXISTS idx_users_role ON users (role);
CREATE INDEX IF NOT EXISTS idx_users_status ON users (status);
CREATE INDEX IF NOT EXISTS idx_users_account_status ON users (account_status);
CREATE INDEX IF NOT EXISTS idx_users_referral_code ON users (referral_code);

CREATE TABLE IF NOT EXISTS otp_codes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  phone VARCHAR(20),
  email VARCHAR(255),
  code VARCHAR(10) NOT NULL,
  purpose VARCHAR(50) CHECK (purpose IN ('login', 'verify_phone', 'verify_email', 'recovery')) DEFAULT 'login',
  expires_at TIMESTAMPTZ NOT NULL,
  used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_otp_codes_phone_code_purpose ON otp_codes (phone, code, purpose);
CREATE INDEX IF NOT EXISTS idx_otp_codes_email_code_purpose ON otp_codes (email, code, purpose);

CREATE TABLE IF NOT EXISTS service_categories (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name VARCHAR(100) NOT NULL,
  slug VARCHAR(100) NOT NULL UNIQUE,
  icon VARCHAR(50),
  parent_id UUID REFERENCES service_categories(id),
  sort_order INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_service_categories_slug ON service_categories (slug);
CREATE INDEX IF NOT EXISTS idx_service_categories_parent_id ON service_categories (parent_id);

CREATE TABLE IF NOT EXISTS audit_logs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  admin_id UUID NOT NULL REFERENCES users(id),
  action VARCHAR(100) NOT NULL,
  entity_type VARCHAR(50) NOT NULL,
  entity_id UUID,
  details TEXT,
  ip_address VARCHAR(45),
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_admin_id ON audit_logs (admin_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_entity_type_entity_id ON audit_logs (entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs (created_at DESC);

CREATE TABLE IF NOT EXISTS sessions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  device_info TEXT,
  ip_address VARCHAR(45),
  last_active_at TIMESTAMPTZ,
  expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions (user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires_at ON sessions (expires_at);

CREATE TABLE IF NOT EXISTS social_accounts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  provider VARCHAR(50) CHECK (provider IN ('google', 'apple', 'linkedin', 'meta')) NOT NULL,
  provider_user_id VARCHAR(255) NOT NULL,
  email VARCHAR(255),
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_social_accounts_user_id ON social_accounts (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_social_accounts_provider_provider_user_id ON social_accounts (provider, provider_user_id);

CREATE TABLE IF NOT EXISTS subscriptions (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  plan VARCHAR(50) CHECK (plan IN ('free', 'plus')) DEFAULT 'free',
  status VARCHAR(50) CHECK (status IN ('active', 'cancelled', 'expired')) DEFAULT 'active',
  payment_provider VARCHAR(50) CHECK (payment_provider IN ('stripe', 'apple', 'google')),
  provider_subscription_id VARCHAR(255),
  started_at TIMESTAMPTZ DEFAULT now(),
  expires_at TIMESTAMPTZ,
  cancelled_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user_id ON subscriptions (user_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_expires_at ON subscriptions (expires_at);

CREATE TABLE IF NOT EXISTS billing_history (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  amount INTEGER NOT NULL,
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'usd',
  description VARCHAR(500) NOT NULL,
  status VARCHAR(50) CHECK (status IN ('paid', 'refunded')) DEFAULT 'paid',
  payment_provider VARCHAR(50) CHECK (payment_provider IN ('stripe', 'apple', 'google')),
  provider_transaction_id VARCHAR(255),
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_billing_history_user_id ON billing_history (user_id);
CREATE INDEX IF NOT EXISTS idx_billing_history_created_at ON billing_history (created_at DESC);

CREATE TABLE IF NOT EXISTS referrals (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  referrer_id UUID NOT NULL REFERENCES users(id),
  referred_id UUID NOT NULL REFERENCES users(id),
  referral_code VARCHAR(50) NOT NULL,
  status VARCHAR(50) CHECK (status IN ('pending', 'completed', 'rewarded')) DEFAULT 'pending',
  reward_type VARCHAR(50) CHECK (reward_type IN ('plus_week')),
  rewarded_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_referrals_referrer_id ON referrals (referrer_id);
CREATE INDEX IF NOT EXISTS idx_referrals_referred_id ON referrals (referred_id);
CREATE INDEX IF NOT EXISTS idx_referrals_referral_code ON referrals (referral_code);

CREATE TABLE IF NOT EXISTS payout_methods (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  type VARCHAR(50) CHECK (type IN ('bank_transfer', 'paypal', 'stripe_connect')) DEFAULT 'bank_transfer',
  details TEXT NOT NULL,
  is_default BOOLEAN DEFAULT FALSE,
  verified BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_payout_methods_user_id ON payout_methods (user_id);
CREATE INDEX IF NOT EXISTS idx_payout_methods_user_id_is_default ON payout_methods (user_id, is_default);

CREATE TABLE IF NOT EXISTS conversations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  type VARCHAR(50) CHECK (type IN ('direct', 'group')) DEFAULT 'direct',
  name VARCHAR(200),
  avatar_url VARCHAR(500),
  created_by UUID REFERENCES users(id),
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_conversations_type ON conversations (type);
CREATE INDEX IF NOT EXISTS idx_conversations_created_by ON conversations (created_by);

CREATE TABLE IF NOT EXISTS reply_templates (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  title VARCHAR(100) NOT NULL,
  content TEXT NOT NULL,
  sort_order INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_reply_templates_user_id ON reply_templates (user_id);
CREATE INDEX IF NOT EXISTS idx_reply_templates_user_id_sort_order ON reply_templates (user_id, sort_order);

CREATE TABLE IF NOT EXISTS connections (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  contact_id UUID NOT NULL REFERENCES users(id),
  status VARCHAR(50) CHECK (status IN ('pending', 'accepted', 'rejected', 'blocked')) DEFAULT 'pending',
  message TEXT,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_connections_user_id ON connections (user_id);
CREATE INDEX IF NOT EXISTS idx_connections_contact_id ON connections (contact_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_connections_user_id_contact_id ON connections (user_id, contact_id);
CREATE INDEX IF NOT EXISTS idx_connections_status ON connections (status);

CREATE TABLE IF NOT EXISTS contact_favorites (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  contact_id UUID NOT NULL REFERENCES users(id),
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_contact_favorites_user_id ON contact_favorites (user_id);

CREATE TABLE IF NOT EXISTS contact_notes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  contact_id UUID NOT NULL REFERENCES users(id),
  note TEXT NOT NULL,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_contact_notes_user_id ON contact_notes (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_notes_user_id_contact_id ON contact_notes (user_id, contact_id);

CREATE TABLE IF NOT EXISTS contact_folders (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  name VARCHAR(100) NOT NULL,
  sort_order INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_contact_folders_user_id ON contact_folders (user_id);

CREATE TABLE IF NOT EXISTS badges (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  badge_type VARCHAR(50) CHECK (badge_type IN ('id_check', 'verified')) NOT NULL,
  status VARCHAR(50) CHECK (status IN ('pending', 'approved', 'rejected')) DEFAULT 'pending',
  selfie_url VARCHAR(500),
  id_front_url VARCHAR(500),
  id_back_url VARCHAR(500),
  match_score NUMERIC(5,2),
  reviewed_by UUID REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_badges_user_id ON badges (user_id);
CREATE INDEX IF NOT EXISTS idx_badges_status ON badges (status);
CREATE INDEX IF NOT EXISTS idx_badges_badge_type_status ON badges (badge_type, status);

CREATE TABLE IF NOT EXISTS user_reports (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  reporter_id UUID NOT NULL REFERENCES users(id),
  reported_id UUID NOT NULL REFERENCES users(id),
  reason VARCHAR(50) CHECK (reason IN ('inappropriate', 'fake', 'harassment', 'no_show', 'scam', 'spam', 'other')) NOT NULL,
  description TEXT,
  evidence TEXT,
  status VARCHAR(50) CHECK (status IN ('pending', 'reviewed', 'resolved', 'dismissed')) DEFAULT 'pending',
  reviewed_by UUID REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_user_reports_reported_id ON user_reports (reported_id);
CREATE INDEX IF NOT EXISTS idx_user_reports_reporter_id ON user_reports (reporter_id);
CREATE INDEX IF NOT EXISTS idx_user_reports_status ON user_reports (status);

CREATE TABLE IF NOT EXISTS provider_profiles (
  user_id UUID PRIMARY KEY REFERENCES users(id),
  profession VARCHAR(200),
  experience_years INTEGER,
  certifications TEXT,
  working_schedule TEXT,
  is_available BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_provider_profiles_user_id ON provider_profiles (user_id);

CREATE TABLE IF NOT EXISTS provider_availability (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  day_of_week INTEGER NOT NULL,
  start_time VARCHAR(10) NOT NULL,
  end_time VARCHAR(10) NOT NULL,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_provider_availability_user_id ON provider_availability (user_id);
CREATE INDEX IF NOT EXISTS idx_provider_availability_user_id_day_of_week ON provider_availability (user_id, day_of_week);

CREATE TABLE IF NOT EXISTS provider_status (
  user_id UUID PRIMARY KEY REFERENCES users(id),
  status VARCHAR(50) CHECK (status IN ('available', 'busy', 'not_available')) DEFAULT 'available',
  until_date TIMESTAMPTZ,
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_provider_status_user_id ON provider_status (user_id);

CREATE TABLE IF NOT EXISTS provider_onboarding (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  step VARCHAR(50) CHECK (step IN ('profile', 'services', 'availability', 'documents', 'verification', 'complete')) DEFAULT 'profile',
  profile_complete BOOLEAN DEFAULT FALSE,
  services_added BOOLEAN DEFAULT FALSE,
  availability_set BOOLEAN DEFAULT FALSE,
  documents_uploaded BOOLEAN DEFAULT FALSE,
  verified BOOLEAN DEFAULT FALSE,
  completed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_onboarding_user_id ON provider_onboarding (user_id);
CREATE INDEX IF NOT EXISTS idx_provider_onboarding_step ON provider_onboarding (step);

CREATE TABLE IF NOT EXISTS calendar_syncs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  provider VARCHAR(50) CHECK (provider IN ('google', 'apple')) DEFAULT 'google',
  access_token VARCHAR(500),
  refresh_token VARCHAR(500),
  token_expires_at TIMESTAMPTZ,
  sync_enabled BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_calendar_syncs_user_id ON calendar_syncs (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_calendar_syncs_user_id_provider ON calendar_syncs (user_id, provider);

CREATE TABLE IF NOT EXISTS user_settings (
  user_id UUID PRIMARY KEY REFERENCES users(id),
  notification_messages BOOLEAN DEFAULT TRUE,
  notification_connections BOOLEAN DEFAULT TRUE,
  notification_appointments BOOLEAN DEFAULT TRUE,
  notification_reviews BOOLEAN DEFAULT TRUE,
  notification_network BOOLEAN DEFAULT FALSE,
  notification_marketing BOOLEAN DEFAULT FALSE,
  privacy_profile_visible BOOLEAN DEFAULT TRUE,
  privacy_show_phone BOOLEAN DEFAULT FALSE,
  privacy_show_location BOOLEAN DEFAULT TRUE,
  privacy_last_seen VARCHAR(50) CHECK (privacy_last_seen IN ('everyone', 'contacts', 'nobody')) DEFAULT 'everyone',
  privacy_read_receipts BOOLEAN DEFAULT TRUE,
  privacy_online_status BOOLEAN DEFAULT TRUE,
  language VARCHAR(50) CHECK (language IN ('en', 'ro', 'ru')) DEFAULT 'en',
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  theme VARCHAR(50) CHECK (theme IN ('light', 'dark', 'system')) DEFAULT 'system',
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_user_settings_user_id ON user_settings (user_id);

CREATE TABLE IF NOT EXISTS blocked_users (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  blocker_id UUID NOT NULL REFERENCES users(id),
  blocked_id UUID NOT NULL REFERENCES users(id),
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_blocked_users_blocker_id ON blocked_users (blocker_id);
CREATE INDEX IF NOT EXISTS idx_blocked_users_blocked_id ON blocked_users (blocked_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_blocked_users_blocker_id_blocked_id ON blocked_users (blocker_id, blocked_id);

CREATE TABLE IF NOT EXISTS notifications (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  type VARCHAR(50) CHECK (type IN ('message_received', 'appointment_proposed', 'appointment_confirmed', 'appointment_cancelled', 'appointment_reminder', 'connection_request', 'connection_accepted', 'review_received', 'contact_joined', 'service_completed', 'dispute_opened', 'welcome', 'badge_reviewed', 'subscription_changed')) NOT NULL,
  title VARCHAR(255) NOT NULL,
  body TEXT,
  data TEXT,
  read BOOLEAN DEFAULT FALSE,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_notifications_user_id ON notifications (user_id);
CREATE INDEX IF NOT EXISTS idx_notifications_user_id_read ON notifications (user_id, read);
CREATE INDEX IF NOT EXISTS idx_notifications_created_at ON notifications (created_at DESC);

CREATE TABLE IF NOT EXISTS device_tokens (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  platform VARCHAR(50) CHECK (platform IN ('ios', 'android', 'web')) NOT NULL,
  token VARCHAR(500) NOT NULL,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_device_tokens_user_id ON device_tokens (user_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_device_tokens_user_id_token ON device_tokens (user_id, token);

CREATE TABLE IF NOT EXISTS gallery_images (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  url VARCHAR(500) NOT NULL,
  caption VARCHAR(500),
  sort_order INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_gallery_images_user_id ON gallery_images (user_id);
CREATE INDEX IF NOT EXISTS idx_gallery_images_user_id_sort_order ON gallery_images (user_id, sort_order);

CREATE TABLE IF NOT EXISTS commission_rules (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name VARCHAR(100) NOT NULL,
  rate NUMERIC(5,4) NOT NULL,
  min_amount NUMERIC(10,2),
  max_amount NUMERIC(10,2),
  category_id UUID REFERENCES service_categories(id),
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_commission_rules_is_active ON commission_rules (is_active);
CREATE INDEX IF NOT EXISTS idx_commission_rules_category_id ON commission_rules (category_id);

CREATE TABLE IF NOT EXISTS user_services (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id UUID NOT NULL REFERENCES users(id),
  category_id UUID NOT NULL REFERENCES service_categories(id),
  service_name VARCHAR(200),
  rate_type VARCHAR(50) CHECK (rate_type IN ('hourly', 'fixed', 'negotiable')) DEFAULT 'negotiable',
  rate_amount NUMERIC(10,2),
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  price_min NUMERIC(10,2),
  price_max NUMERIC(10,2),
  description TEXT,
  is_active BOOLEAN DEFAULT TRUE,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_user_services_user_id ON user_services (user_id);
CREATE INDEX IF NOT EXISTS idx_user_services_category_id ON user_services (category_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_services_user_id_category_id ON user_services (user_id, category_id);

CREATE TABLE IF NOT EXISTS payouts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  provider_id UUID NOT NULL REFERENCES users(id),
  payout_method_id UUID NOT NULL REFERENCES payout_methods(id),
  amount NUMERIC(10,2) NOT NULL,
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  status VARCHAR(50) CHECK (status IN ('requested', 'processing', 'completed', 'failed')) DEFAULT 'requested',
  reference VARCHAR(255),
  failure_reason TEXT,
  requested_at TIMESTAMPTZ DEFAULT now(),
  processed_at TIMESTAMPTZ,
  completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_payouts_provider_id ON payouts (provider_id);
CREATE INDEX IF NOT EXISTS idx_payouts_status ON payouts (status);
CREATE INDEX IF NOT EXISTS idx_payouts_provider_id_status ON payouts (provider_id, status);
CREATE INDEX IF NOT EXISTS idx_payouts_requested_at ON payouts (requested_at DESC);

CREATE TABLE IF NOT EXISTS conversation_participants (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id UUID NOT NULL REFERENCES conversations(id),
  user_id UUID NOT NULL REFERENCES users(id),
  role VARCHAR(50) CHECK (role IN ('admin', 'member')) DEFAULT 'member',
  last_read_at TIMESTAMPTZ,
  archived_at TIMESTAMPTZ,
  joined_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_conversation_participants_user_id ON conversation_participants (user_id);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_conversation_id ON conversation_participants (conversation_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_conversation_participants_conversation_id_user_id ON conversation_participants (conversation_id, user_id);

CREATE TABLE IF NOT EXISTS messages (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id UUID NOT NULL REFERENCES conversations(id),
  sender_id UUID NOT NULL REFERENCES users(id),
  type VARCHAR(50) CHECK (type IN ('text', 'photo', 'video', 'audio', 'document', 'location', 'contact_share', 'appointment_card', 'service_card')) DEFAULT 'text',
  content TEXT NOT NULL,
  data TEXT,
  reply_to_id UUID REFERENCES messages(id),
  edited_at TIMESTAMPTZ,
  deleted_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation_id_created_at ON messages (conversation_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_sender_id ON messages (sender_id);
CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON messages (conversation_id);

CREATE TABLE IF NOT EXISTS appointments (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  client_id UUID NOT NULL REFERENCES users(id),
  provider_id UUID NOT NULL REFERENCES users(id),
  title VARCHAR(200) NOT NULL,
  start_at TIMESTAMPTZ NOT NULL,
  end_at TIMESTAMPTZ NOT NULL,
  location VARCHAR(500),
  services TEXT,
  status VARCHAR(50) CHECK (status IN ('proposed', 'confirmed', 'in_progress', 'completed', 'dismissed', 'cancelled', 'no_show')) DEFAULT 'proposed',
  notes TEXT,
  proposed_by UUID NOT NULL REFERENCES users(id),
  dismiss_reason TEXT,
  cancel_reason TEXT,
  conversation_id UUID REFERENCES conversations(id),
  completion_amount NUMERIC(10,2),
  completion_currency VARCHAR(50) CHECK (completion_currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  completion_hours NUMERIC(6,2),
  completion_confirmed_at TIMESTAMPTZ,
  completion_auto_confirm_at TIMESTAMPTZ,
  no_show_count INTEGER DEFAULT 0,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_appointments_client_id ON appointments (client_id);
CREATE INDEX IF NOT EXISTS idx_appointments_provider_id ON appointments (provider_id);
CREATE INDEX IF NOT EXISTS idx_appointments_status ON appointments (status);
CREATE INDEX IF NOT EXISTS idx_appointments_start_at ON appointments (start_at);
CREATE INDEX IF NOT EXISTS idx_appointments_client_id_status ON appointments (client_id, status);
CREATE INDEX IF NOT EXISTS idx_appointments_provider_id_status ON appointments (provider_id, status);
CREATE INDEX IF NOT EXISTS idx_appointments_conversation_id ON appointments (conversation_id);

CREATE TABLE IF NOT EXISTS contact_folder_members (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  folder_id UUID NOT NULL REFERENCES contact_folders(id),
  contact_id UUID NOT NULL REFERENCES users(id)
);

CREATE INDEX IF NOT EXISTS idx_contact_folder_members_folder_id ON contact_folder_members (folder_id);

CREATE TABLE IF NOT EXISTS message_read_receipts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  message_id UUID NOT NULL REFERENCES messages(id),
  user_id UUID NOT NULL REFERENCES users(id),
  read_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_message_read_receipts_message_id ON message_read_receipts (message_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_message_read_receipts_message_id_user_id ON message_read_receipts (message_id, user_id);

CREATE TABLE IF NOT EXISTS service_history (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  appointment_id UUID NOT NULL REFERENCES appointments(id),
  client_id UUID NOT NULL REFERENCES users(id),
  provider_id UUID NOT NULL REFERENCES users(id),
  services TEXT,
  hours_worked NUMERIC(6,2),
  rate NUMERIC(10,2),
  total_amount NUMERIC(10,2),
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  status VARCHAR(50) CHECK (status IN ('confirmed', 'disputed')) DEFAULT 'confirmed',
  completed_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_service_history_client_id ON service_history (client_id);
CREATE INDEX IF NOT EXISTS idx_service_history_provider_id ON service_history (provider_id);
CREATE INDEX IF NOT EXISTS idx_service_history_appointment_id ON service_history (appointment_id);
CREATE INDEX IF NOT EXISTS idx_service_history_completed_at ON service_history (completed_at DESC);

CREATE TABLE IF NOT EXISTS reviews (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  appointment_id UUID NOT NULL REFERENCES appointments(id),
  reviewer_id UUID NOT NULL REFERENCES users(id),
  reviewee_id UUID NOT NULL REFERENCES users(id),
  rating INTEGER NOT NULL,
  content TEXT,
  tags TEXT,
  photos TEXT,
  response TEXT,
  response_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_reviews_reviewee_id ON reviews (reviewee_id);
CREATE INDEX IF NOT EXISTS idx_reviews_reviewer_id ON reviews (reviewer_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_appointment_id_reviewer_id ON reviews (appointment_id, reviewer_id);
CREATE INDEX IF NOT EXISTS idx_reviews_reviewee_id_rating ON reviews (reviewee_id, rating);

CREATE TABLE IF NOT EXISTS disputes (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  appointment_id UUID NOT NULL REFERENCES appointments(id),
  opened_by UUID NOT NULL REFERENCES users(id),
  reason TEXT NOT NULL,
  proposed_amount NUMERIC(10,2),
  counter_amount NUMERIC(10,2),
  status VARCHAR(50) CHECK (status IN ('open', 'resolved', 'expired')) DEFAULT 'open',
  resolution TEXT,
  resolved_at TIMESTAMPTZ,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now(),
  updated_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_disputes_appointment_id ON disputes (appointment_id);
CREATE INDEX IF NOT EXISTS idx_disputes_opened_by ON disputes (opened_by);
CREATE INDEX IF NOT EXISTS idx_disputes_status ON disputes (status);

CREATE TABLE IF NOT EXISTS appointment_calendar_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  appointment_id UUID NOT NULL REFERENCES appointments(id),
  user_id UUID NOT NULL REFERENCES users(id),
  external_event_id VARCHAR(255) NOT NULL,
  calendar_provider VARCHAR(50) CHECK (calendar_provider IN ('google', 'apple')) DEFAULT 'google',
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_appointment_calendar_events_appointment_id ON appointment_calendar_events (appointment_id);
CREATE INDEX IF NOT EXISTS idx_appointment_calendar_events_user_id ON appointment_calendar_events (user_id);
CREATE INDEX IF NOT EXISTS idx_appointment_calendar_events_external_event_id ON appointment_calendar_events (external_event_id);

CREATE TABLE IF NOT EXISTS provider_earnings (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  provider_id UUID NOT NULL REFERENCES users(id),
  appointment_id UUID NOT NULL REFERENCES appointments(id),
  service_history_id UUID NOT NULL REFERENCES service_history(id),
  gross_amount NUMERIC(10,2) NOT NULL,
  commission_rate NUMERIC(5,4) DEFAULT 0.10,
  commission_amount NUMERIC(10,2) NOT NULL,
  net_amount NUMERIC(10,2) NOT NULL,
  currency VARCHAR(50) CHECK (currency IN ('eur', 'mdl', 'usd')) DEFAULT 'eur',
  status VARCHAR(50) CHECK (status IN ('pending', 'available', 'paid_out')) DEFAULT 'pending',
  available_at TIMESTAMPTZ,
  paid_out_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_provider_earnings_provider_id ON provider_earnings (provider_id);
CREATE INDEX IF NOT EXISTS idx_provider_earnings_appointment_id ON provider_earnings (appointment_id);
CREATE INDEX IF NOT EXISTS idx_provider_earnings_service_history_id ON provider_earnings (service_history_id);
CREATE INDEX IF NOT EXISTS idx_provider_earnings_status ON provider_earnings (status);
CREATE INDEX IF NOT EXISTS idx_provider_earnings_provider_id_status ON provider_earnings (provider_id, status);
CREATE INDEX IF NOT EXISTS idx_provider_earnings_created_at ON provider_earnings (created_at DESC);

