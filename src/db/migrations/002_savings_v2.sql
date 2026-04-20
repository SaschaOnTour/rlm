-- Migration 002: savings v2 — per-operation token and call breakdown.
--
-- Extends the savings table with four additional columns so reports can
-- distinguish rlm call count / tokens from the hypothetical alternative
-- call count / tokens, instead of only logging a single scalar.

ALTER TABLE savings ADD COLUMN rlm_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE savings ADD COLUMN alt_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE savings ADD COLUMN rlm_calls INTEGER NOT NULL DEFAULT 1;
ALTER TABLE savings ADD COLUMN alt_calls INTEGER NOT NULL DEFAULT 1;
