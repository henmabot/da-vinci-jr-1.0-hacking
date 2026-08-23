## Davinci Jr. 1.0 Hardware

Components that I was able to identify:

- 4x Stepper motors
- 1x Filament sensor
- 1x RFID reader
- 3x Optical endstops
- 1x Top light bar

I only identified the ones that need to be defined in the firmware.

### Stepper Motors

They seem to be Nema 17's. There are 4 of them, one for each axis (X, Y, Z, and E1).

In this photo the one on the left is E1, the one in the back is Z, and the other two are Y and X. Y and X seem to have the same head.
![All steppers after removal (YT)](images/steppers.png)

X motor is on the left side and moves along the Z axis, and stays parallel to the print bed.
![Back view (YT)](images/x-motor.png)

Y motor sits in the middle of the print bed, slightly to the right. It's fixed in the place, and sits perpendicular to the print bed.
![Back view (YT)](images/y-motor.png)

Z motor sits parallel to the Y motor, and is also fixed in the place.
![Back view (YT)](images/z-motor.png)

E1 motor sits on the top left side and is perpendicular to the print bed, is fixed in place.
![Front view (YT)](images/e1-motor-1.png)
![Side view (YT)](images/e1-motor-2.png)

## Sources:

- [Teardown (YouTube)](https://www.youtube.com/watch?v=cn2mYWmanlk)
- [julialongtin (GitHub)](https://github.com/julialongtin/Davinci_Jr_Hacking)
- My own disassembly and photos of my unit
