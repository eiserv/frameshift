export function Badge({ label }: { label: string }) {
  if (!label) {
    return null;
  }
  return <span className="badge">{label}</span>;
}
