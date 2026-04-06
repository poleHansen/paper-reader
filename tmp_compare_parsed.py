import json
from pathlib import Path

paths = [
    Path(r'C:\Users\honor\AppData\Roaming\com.paperreader.app\parsed\paper_5a69b88f070b4fdb81990fdea4cc2630.json'),
    Path(r'C:\Users\honor\AppData\Roaming\com.paperreader.app\parsed\paper_2f3b247ac6aa427cbf7d30173a4fd038.json'),
]
for path in paths:
    data = json.loads(path.read_text(encoding='utf-8'))
    print(f'FILE={path.name}')
    print('figures', len(data.get('figures', [])))
    print('tables', len(data.get('tables', [])))
    print('visual_mode', data.get('metadata', {}).get('visualParsing', {}).get('mode'))
    print('figure_labels', [item.get('label') for item in data.get('figures', [])])
    print('table_labels', [item.get('label') for item in data.get('tables', [])])
    print('warnings', data.get('metadata', {}).get('visualParsing', {}).get('warnings', []))
    print('diagnostic_count', len(data.get('metadata', {}).get('visualParsing', {}).get('diagnostics', [])))
    print('-' * 80)
