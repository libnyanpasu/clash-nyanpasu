// Next version e.g. 1.12.3
module.exports = exports = {};
module.exports.preVersionGeneration = function preVersionGeneration(version) {
  return process.env.NYANPASU_VERSION;
};

// Next tag e.g. v1.12.3
module.exports.preTagGeneration = function preTagGeneration(tag) {
  return `v${process.env.NYANPASU_VERSION}`;
};
