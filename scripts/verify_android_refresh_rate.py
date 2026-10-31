"""Run the production refresh selection policy without Android or a device."""
from pathlib import Path
import subprocess
import tempfile
from verify_android_multitouch import executable


def main():
    root = Path(__file__).resolve().parent.parent
    package = Path('org/flos/phirapro')
    source = root / 'phira-android/app/src/main/java' / package / 'RefreshRatePolicy.java'
    request = source.with_name('RefreshRateRequest.java')
    regression = root / 'phira-android/app/src/test/java' / package / 'RefreshRatePolicyRegression.java'
    with tempfile.TemporaryDirectory(prefix='phira-refresh-') as classes:
        subprocess.run([executable('javac'), '-encoding', 'UTF-8', '--release', '8', '-Xlint:-options',
                        '-d', classes, str(source), str(request), str(regression)], check=True)
        subprocess.run([executable('java'), '-cp', classes,
                        'org.flos.phirapro.RefreshRatePolicyRegression'], check=True)


if __name__ == '__main__':
    main()
