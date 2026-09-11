import type { Env } from './env';
declare global {
  namespace JSX {
    interface IntrinsicElements {
      button: { children?: unknown };
      input: { value?: string };
    }
  }
}
export const View = <T extends Env,>({ token }: T) => <button><input value={token} /></button>;
const text = '<select /><textarea />';
// <button />
