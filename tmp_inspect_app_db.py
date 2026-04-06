import json
import os
import sqlite3

appdata = os.path.join(os.environ['APPDATA'], 'com.paperreader.app')
db = os.path.join(appdata, 'app.db')
print(f'APPDATA={appdata}')
print(f'DB={db}')
if not os.path.exists(db):
    raise SystemExit('db_not_found')

conn = sqlite3.connect(db)
conn.row_factory = sqlite3.Row
rows = conn.execute(
    '''
    SELECT p.id, p.title, p.updated_at, ppa.storage_path, ppa.figure_count, ppa.table_count, ppa.visual_mode, ppa.visual_summary_count
    FROM papers p
    LEFT JOIN parsed_paper_artifacts ppa ON ppa.paper_id = p.id
    ORDER BY p.updated_at DESC
    LIMIT 10
    '''
).fetchall()
for row in rows:
    print(json.dumps(dict(row), ensure_ascii=False))
