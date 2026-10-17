export type DiffOp = { type: "equal" | "delete" | "insert"; text: string };

// Tokens: runs of whitespace, or runs of non-whitespace. Whitespace is kept so ops reconstruct both strings exactly.
const tokenize = (s: string) => s.match(/\s+|\S+/g) ?? [];

export function diffWords(a: string, b: string): DiffOp[] {
  const x = tokenize(a);
  const y = tokenize(b);
  const n = x.length, m = y.length;
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = x[i] === y[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);

  const ops: DiffOp[] = [];
  const push = (type: DiffOp["type"], text: string) => {
    const last = ops[ops.length - 1];
    if (last && last.type === type) last.text += text;
    else ops.push({ type, text });
  };
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) { push("equal", x[i]); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) push("delete", x[i++]);
    else push("insert", y[j++]);
  }
  while (i < n) push("delete", x[i++]);
  while (j < m) push("insert", y[j++]);
  return ops;
}
