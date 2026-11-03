import { binaryNoiseMask, fitNoiseRectangles } from './noise-fit.mjs';

self.onmessage = ({ data }) => {
  try {
    const mask = binaryNoiseMask(data.pixels, data.width, data.height, data.options);
    const result = fitNoiseRectangles(mask, data.width, data.height, data.options);
    self.postMessage({ result });
  } catch (error) { self.postMessage({ error: error.message }); }
};
