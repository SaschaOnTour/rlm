package main

// Missing import quote
import fmt

func broken( {
    // missing parameter close paren
    fmt.Println("hello")
}

type Incomplete struct {
    Name string
    // missing closing brace
