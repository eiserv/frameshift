function read() {
  return typeof shared;
}

export default function CapturedPage() {
  const shared = read();
  return <p>{`${shared}`}</p>;
}
