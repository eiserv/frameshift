function makeStamp() {
  return 'Edition 1';
}

const stamp = makeStamp();

export function Stamp() {
  return <small>{`${stamp}`}</small>;
}
