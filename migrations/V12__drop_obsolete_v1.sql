-- V12: Drop obsolete V1 tables after all Stage 0 V2 tables exist.
-- This migration runs AFTER V2..V11 have created the new Stage 0 schema.
-- PRAGMA foreign_keys=OFF avoids ordering issues between inter-dependent V1 tables.
-- Do NOT drop the retained V1 `artifacts` table here.

PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS campaigns;
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS claims;
DROP TABLE IF EXISTS evidences;
DROP TABLE IF EXISTS leases;
DROP TABLE IF EXISTS functions;
DROP TABLE IF EXISTS modules;
DROP TABLE IF EXISTS binary_revisions;

PRAGMA foreign_keys = ON;
