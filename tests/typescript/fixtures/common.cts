import runtime = require('./env');
import type types = require('./env');
declare function require(name: string): unknown;
const ordinary = require('./env');
export const value = runtime.value;
export type Config = types.Env;
