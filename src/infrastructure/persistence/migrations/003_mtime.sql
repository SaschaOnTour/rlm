-- Migration 003: files.mtime_nanos for the staleness detector.
--
-- Stores the file's own mtime at index time in nanoseconds since the
-- Unix epoch. Used by the staleness detector to skip hashing unchanged
-- files via exact equality with the on-disk mtime. Nanosecond precision
-- (as provided by modern filesystems — ext4 / NTFS / APFS / btrfs)
-- makes the comparison safe even when edit → index → edit all happen
-- within the same wall-clock second. Legacy rows have mtime_nanos = 0
-- (sentinel), which forces a hash verification on the first staleness
-- check.

ALTER TABLE files ADD COLUMN mtime_nanos INTEGER NOT NULL DEFAULT 0;
