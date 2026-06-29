// A small Go example: a temperature converter.

package main

import (
	"fmt"
	"math"
)

type Temperature struct {
	Celsius float64
}

func (t Temperature) Fahrenheit() float64 {
	return t.Celsius*9.0/5.0 + 32.0
}

func (t Temperature) Kelvin() float64 {
	return t.Celsius + 273.15
}

func round2(v float64) float64 {
	return math.Round(v*100) / 100
}

func main() {
	temps := []Temperature{{0}, {20}, {37}, {100}}
	for _, t := range temps {
		fmt.Printf("%.1f°C = %.2f°F = %.2fK\n",
			t.Celsius, round2(t.Fahrenheit()), round2(t.Kelvin()))
	}
}
