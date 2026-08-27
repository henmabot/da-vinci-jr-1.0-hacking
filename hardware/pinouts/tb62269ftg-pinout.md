# TB62269FTG 48-Lead WQFN Pinout

| Pin | Pin Name | Description                                | Pin | Pin Name           | Description                                  |
| --- | -------- | ------------------------------------------ | --- | ------------------ | -------------------------------------------- |
| 01  | NC       | No-connect                                 | 25  | NC                 | No-connect                                   |
| 02  | CLK_IN   | Clock input, important for motor frequency | 26  | OUT_B2*            | Bch positive driver output                   |
| 03  | ENABLE   | A/B channel output enable                  | 27  | OUT_B1*            |
| 04  | RESET    | Electric angle reset                       | 28  | NC                 | No-connect                                   |
| 05  | GND      | Logic ground                               | 29  | RS_B2*             | Motor Bch current sense pin                  |
| 06  | NC       | No-connect                                 | 30  | RS_B1*             |                                              |
| 07  | RS_A1*   | Motor Ach current sense pin                | 31  | NC                 | No-connect                                   |
| 08  | RS_A2*   | 32                                         | VM  | Motor Power supply |
| 09  | NC       | No-connect                                 | 33  | NC                 | No-connect                                   |
| 10  | OUT_A1*  | Ach positive driver output                 | 34  | VCC                | Internal VCC regulator monitor pin           |
| 11  | OUT_A2*  | 35                                         | NC  | No-connect         |
| 12  | NC       | No-connect                                 | 36  | NC                 | No-connect                                   |
| 13  | NC       | No-connect                                 | 37  | NC                 | No-connect                                   |
| 14  | NC       | No-connect                                 | 38  | L_OUT              | Error detect signal output                   |
| 15  | GND      | Motor power ground                         | 39  | D_MODE0            | Step resolution mode control 0               |
| 16  | OUT_A1-* | Ach negative driver output                 | 40  | GND                | Logic ground                                 |
| 17  | OUT_A2-* | Ach negative driver output                 | 41  | VREF_B             | Tunes the current level for Bch motor drive. |
| 18  | GND      | Motor power ground                         | 42  | VREF_A             | Tunes the current level for Ach motor drive. |
| 19  | GND      | Motor power ground                         | 43  | OSCM               | Oscillator pin for PWM chopper               |
| 20  | OUT_B2-* | Bch negative driver output                 | 44  | CW/CCW             | Motor rotation: forward/reverse              |
| 21  | OUT_B1-* | Bch negative driver output                 | 45  | MO_OUT             | Electric angle monitor                       |
| 22  | GND      | Motor power ground                         | 46  | D_MODE1            | Step resolution mode control 1               |
| 23  | NC       | No-connect                                 | 47  | D_MODE2            | Step resolution mode control 2               |
| 24  | NC       | No-connect                                 | 48  | NC                 | No-connect                                   |

- Source: [Toshiba TB62269FTG Datasheet](../../SOURCES.md#datasheet-toshiba-tb62269ftg)
