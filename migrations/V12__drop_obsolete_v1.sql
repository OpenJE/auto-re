-- V12: Drop obsolete V1 tables after all Stage 0 V2 tables exist.
-- This migration runs AFTER V2..V11 have created the new Stage 0 schema.
--
-- Tables are dropped in dependency order (referencing tables first) because
-- refinery runs each migration inside a transaction, where `PRAGMA foreign_keys`
-- cannot be toggled. The retained V1 `artifacts` table is NOT dropped here.

DROP TABLE IF EXISTS functions;
DROP TABLE IF EXISTS modules;
DROP TABLE IF EXISTS binary_revisions;
DROP TABLE IF EXISTS leases;
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS campaigns;
DROP TABLE IF EXISTS claims;
DROP TABLE IF EXISTS evidences;
