/** @format */

import { useRef, useState } from "react";
import { motion } from "motion/react";

export interface IButtonProps {
  /** An async handler keeps the button disabled until its promise settles. */
  onClick: () => void | Promise<unknown>;
  onBlur?: () => void;
  className?: string;
  children?: React.ReactNode;
  fullRound?: boolean;
  animate?: boolean;
  disabled?: boolean;
  title?: string;
  "aria-label"?: string;
}
export default function Button({
  onClick,
  onBlur,
  children,
  className,
  fullRound,
  animate,
  disabled,
  title,
  "aria-label": ariaLabel,
}: IButtonProps) {
  const fullRoundClass = fullRound ? "rounded-full" : "rounded-4xl";
  // The ref closes the gap between a second click and the re-render that
  // disables the button, so a double click can never run the action twice.
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const handleClick = () => {
    if (pendingRef.current) {
      return;
    }
    const result = onClick();
    if (result instanceof Promise) {
      pendingRef.current = true;
      setPending(true);
      void result
        .finally(() => {
          pendingRef.current = false;
          setPending(false);
        })
        .catch(console.error);
    }
  };
  return (
    <motion.button
      onClick={handleClick}
      onBlur={onBlur}
      disabled={disabled || pending}
      title={title}
      aria-label={ariaLabel}
      whileHover={animate ? { y: -5, scale: 1.1 } : {}}
      whileTap={{ scale: 0.9 }}
      transition={{ duration: 0.15 }}
      className={
        fullRoundClass +
        " p-2 font-extrabold hover:cursor-pointer disabled:opacity-60 " +
        className +
        " "
      }
    >
      {children}
    </motion.button>
  );
}
