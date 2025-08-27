import os
import sys
import uuid
import random

path = './test/'
n = 20
m = 50

if os.path.exists(path):
    print('./test exists. Remove the folder first.', file=sys.stderr)
    exit(1)

os.mkdir(path)

for i in range(n):
    sub = path + f'{i:03d}/'
    os.mkdir(sub)
    for j in range(m):
        fp = sub + f'{j:03d}.adoc'
        with open(fp, 'w', encoding='utf-8') as f:
            mth = random.randint(1, 12)
            day = random.randint(1, 27)
            date = f'2025-{mth:02d}-{day:02d}'

            f.write(f'= {uuid.uuid1()}\n')
            f.write(f':revdate: {date}\n')
            f.write(f'\n')
            f.write(f'Revdate: {date}\n')
