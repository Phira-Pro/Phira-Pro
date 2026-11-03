import { showDialog } from './dialog.mjs';
import { t } from '../i18n.mjs';

const node = (tag, text) => { const element = document.createElement(tag); element.textContent = text; return element; };
const steps = (host, title, items) => { host.append(node('h3', title)); const list = node('ol', ''); for (const text of items) list.append(node('li', text)); host.append(list); };

export function showAudioSeparationGuide(desktop) {
  const host = showDialog(t('audio-separation-guide.s226a312'), t('audio-separation-guide.sf3ce522')); host.classList.add('audio-separation-guide');
  host.closest('dialog').addEventListener('close', () => host.classList.remove('audio-separation-guide'), { once: true });
  if (desktop) steps(host, t('audio-separation-guide.scdf2bd2'), [
    t('audio-separation-guide.sdbaa6ec'),
    t('audio-separation-guide.s5b6bf9a'),
    t('audio-separation-guide.sd965f44'),
    t('audio-separation-guide.s586c036'),
    t('audio-separation-guide.se7e62d3'),
  ]);
  else steps(host, t('audio-separation-guide.sd504b19'), [
    t('audio-separation-guide.sab952a0'),
    t('audio-separation-guide.sab22037'),
    t('audio-separation-guide.sc7cf867'),
    t('audio-separation-guide.sbe863fd'),
    t('audio-separation-guide.s984d1f8'),
  ]);
  const other = node('details', ''); other.append(node('summary', desktop ? t('audio-separation-guide.s775a104') : t('audio-separation-guide.s4fe94af')));
  other.append(node('p', desktop ? t('audio-separation-guide.s6f61f07') : t('audio-separation-guide.sfa0c709'))); host.append(other);
  steps(host, t('audio-separation-guide.s9fc3a43'), [
    t('audio-separation-guide.se2383c0'),
    t('audio-separation-guide.s82bc9cd'),
    t('audio-separation-guide.sec2c246'),
    t('audio-separation-guide.sc334334'),
    ...(desktop ? [t('audio-separation-guide.se8c1f1d')] : []),
  ]);
  host.append(node('p', t('audio-separation-guide.s7bdb25d')));
}
