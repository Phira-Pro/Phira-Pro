export function assetUrl(path, moduleUrl = import.meta.url) {
  return new URL(`../../assets/${path}`, moduleUrl).href;
}
