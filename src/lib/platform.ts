/** Running in the Android app (docs/android-design.md). Desktop-only parts of the UI check this. */
export const isMobile = /Android/i.test(navigator.userAgent);
