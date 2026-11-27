-- Artifacts are called quilts.
ALTER TABLE artifacts RENAME TO quilts;
ALTER TABLE artifact_versions RENAME TO quilt_versions;
ALTER TABLE quilt_versions RENAME COLUMN artifact_id TO quilt_id;
