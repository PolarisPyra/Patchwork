-- Whether the person pinned an artifact, which keeps it first in the gallery.
ALTER TABLE artifacts ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
