let counter = 0;

/** @type {import("@rspack/core").LoaderDefinition} */
module.exports = function () {
	return `module.exports = ${counter++};`;
};
