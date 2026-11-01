'use client';

import { useState } from 'react';

export default function ContactPage() {
  const [sent, setSent] = useState(false);
  return <button onClick={() => setSent(true)}>{sent ? 'Thanks' : 'Send'}</button>;
}
