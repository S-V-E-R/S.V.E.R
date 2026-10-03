#!/bin/bash
# S.V.E.R 2.0 rebuild database nightly backup.
# Dumps the sver-rebuild-stage Postgres (the database serving sver.tv), validates
# the archive, writes a checksum and metadata, and prunes local copies.
#
# Output goes to a SUBDIRECTORY of the legacy backup directory so the existing
# NAS pull (rrsync -ro /opt/sver/backups/postgres) copies it offsite, while the
# legacy restore rehearsal (maxdepth 1) never mistakes it for a legacy dump.
# Files are root:ubuntu 0640 because the NAS pull authenticates as ubuntu.
#
# Restoring needs the matching encryption key from /opt/sver-rebuild/shared,
# which is NOT in this dump. Back that key up separately.
set -euo pipefail
umask 027

PROJECT="${COMPOSE_PROJECT:-sver-rebuild-stage}"
SERVICE="${POSTGRES_SERVICE:-postgres}"
DB_BACKUP_DIR="${DB_BACKUP_DIR:-/opt/sver/backups/postgres/rebuild}"
DB_BACKUP_RETENTION_DAYS="${DB_BACKUP_RETENTION_DAYS:-7}"
READER_GROUP="${READER_GROUP:-ubuntu}"

if ! [[ "$DB_BACKUP_RETENTION_DAYS" =~ ^[0-9]+$ ]]; then
  echo "ERROR: DB_BACKUP_RETENTION_DAYS must be a whole number"; exit 1
fi

mapfile -t ids < <(docker ps -q \
  --filter "label=com.docker.compose.project=$PROJECT" \
  --filter "label=com.docker.compose.service=$SERVICE")
if [ "${#ids[@]}" -ne 1 ]; then
  echo "ERROR: expected exactly one running $PROJECT/$SERVICE container, found ${#ids[@]}"; exit 1
fi
cid="${ids[0]}"

install -d -m 0750 -o root -g "$READER_GROUP" "$DB_BACKUP_DIR"

TS="$(date -u +%Y%m%dT%H%M%SZ)"
NAME="postgres-${TS}.dump"
FINAL="$DB_BACKUP_DIR/$NAME"
PARTIAL="$DB_BACKUP_DIR/.${NAME}.partial"
trap 'rm -f "$PARTIAL"' EXIT

echo "Rebuild Postgres backup started: $(date -u)"
echo "  Container: $PROJECT/$SERVICE ($cid)"
echo "  Backup:    $FINAL"

docker exec "$cid" sh -c '
  export PGPASSWORD="${POSTGRES_PASSWORD:-}"
  pg_dump --format=custom --no-owner --no-privileges --compress=9 \
    -U "$POSTGRES_USER" -d "$POSTGRES_DB"
' > "$PARTIAL"

[ -s "$PARTIAL" ] || { echo "ERROR: dump is empty"; exit 1; }
docker exec -i "$cid" pg_restore --list < "$PARTIAL" > /dev/null \
  || { echo "ERROR: pg_restore could not read the archive"; exit 1; }

mv "$PARTIAL" "$FINAL"
( cd "$DB_BACKUP_DIR" && sha256sum "$NAME" > "$NAME.sha256" && sha256sum -c "$NAME.sha256" )
{
  echo "created_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "backup_file=$NAME"
  echo "size_bytes=$(wc -c < "$FINAL" | tr -d ' ')"
  echo "compose_project=$PROJECT"
  echo "postgres_service=$SERVICE"
  echo "archive_validation=pg_restore_list_passed"
  echo "release=$(readlink -f /opt/sver-rebuild/current 2>/dev/null || echo unknown)"
  echo "encryption_key=not_included_backup_separately"
} > "$FINAL.meta"
chown root:"$READER_GROUP" "$FINAL" "$FINAL.sha256" "$FINAL.meta"
chmod 0640 "$FINAL" "$FINAL.sha256" "$FINAL.meta"

find "$DB_BACKUP_DIR" -maxdepth 1 -type f \( -name 'postgres-*.dump' -o \
  -name 'postgres-*.dump.sha256' -o -name 'postgres-*.dump.meta' \) \
  -mtime +"$DB_BACKUP_RETENTION_DAYS" -delete

echo "Backup complete: $(du -h "$FINAL" | cut -f1), sha256 $(cut -d' ' -f1 "$FINAL.sha256"), retention ${DB_BACKUP_RETENTION_DAYS} days"