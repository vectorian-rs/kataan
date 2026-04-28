export async function health() {
  const response = await fetch('http://127.0.0.1:3001/api/health');
  if (!response.ok) throw new Error(`Health check failed: ${response.status}`);
  return response.json() as Promise<{ ok: boolean }>;
}
