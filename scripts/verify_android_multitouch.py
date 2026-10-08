"""Execute production Java pointer routing against 3..10-finger regressions.

Requires a JDK, not Android instrumentation or third-party test packages.
This verifies event translation, not OEM system gestures or physical touch hardware.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def executable(name):
    home = os.environ.get('JAVA_HOME')
    if home:
        candidate = Path(home) / 'bin' / (name + ('.exe' if os.name == 'nt' else ''))
        if candidate.is_file():
            return str(candidate)
    found = shutil.which(name)
    if not found:
        raise SystemExit(f'{name} unavailable; set JAVA_HOME to a JDK')
    return found


def main():
    root = Path(__file__).resolve().parent.parent
    package = Path('org/flos/phirapro')
    source = root / 'phira-android/app/src/main/java' / package / 'TouchEventRouter.java'
    regression = root / 'phira-android/app/src/test/java' / package / 'TouchEventRouterRegression.java'
    with tempfile.TemporaryDirectory(prefix='phira-multitouch-') as classes:
        subprocess.run([executable('javac'), '-encoding', 'UTF-8', '--release', '8', '-Xlint:-options',
                        '-d', classes, str(source), str(regression)], check=True)
        subprocess.run([executable('java'), '-cp', classes,
                        'org.flos.phirapro.TouchEventRouterRegression'], check=True)


if __name__ == '__main__':
    main()
